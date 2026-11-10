//! A read-only Git checkout whose commands run through an SSH session.

use std::collections::HashMap;

use anyhow::{Context, Result};
use git2::Oid;

use crate::{ssh::Session, ChangeKind, Commit, FileChange, Reader, RefEntry, RefKind, RefLabel, WorkingState};

/// A remote checkout. Its session remains open for as long as this value does.
pub struct RemoteRepo {
    session: Session,
    path: String,
}

impl RemoteRepo {
    pub fn new(session: Session, path: String) -> Result<Self> {
        session.verify_checkout(&path)?;
        Ok(Self { session, path })
    }

    fn git(&self, arguments: &[&str]) -> Result<String> {
        let mut words = vec!["git", "-C", &self.path];
        words.extend(arguments);
        self.session.command(&words)
    }

    fn ref_labels(&self) -> Result<HashMap<Oid, Vec<RefLabel>>> {
        let mut labels = HashMap::new();
        let output = self.git(&["for-each-ref", "--format=%(objectname)%00%(refname)%00"])?;
        let fields: Vec<&str> = output.split('\0').collect();
        for pair in fields.chunks_exact(2) {
            let Ok(target) = Oid::from_str(pair[0]) else { continue };
            let Some((kind, name)) = label(pair[1]) else { continue };
            labels.entry(target).or_insert_with(Vec::new).push(RefLabel { name, kind });
        }
        Ok(labels)
    }
}

impl Reader for RemoteRepo {
    fn location(&self) -> String {
        format!("{}@{}:{}", self.session.login().user, self.session.login().host, self.path)
    }

    fn head_name(&self) -> String {
        self.git(&["symbolic-ref", "--quiet", "--short", "HEAD"])
            .ok()
            .map(|name| name.trim().to_string())
            .unwrap_or_else(|| "detached HEAD".to_string())
    }

    fn commits(&self, limit: usize) -> Result<Vec<Commit>> {
        let labels = self.ref_labels()?;
        let output = self.git(&[
            "log", "--all", "--topo-order", "--date-order", "--format=%H%x1f%h%x1f%an%x1f%at%x1f%P%x1f%s%x1e",
            &format!("-{limit}"),
        ])?;
        let head = self.head_name();
        output
            .split('\x1e')
            .filter(|row| !row.trim().is_empty())
            .map(|row| remote_commit(row, &labels, &head))
            .collect()
    }

    fn references(&self) -> Result<Vec<RefEntry>> {
        let head = self.head_name();
        let mut refs = self
            .ref_labels()?
            .into_iter()
            .flat_map(|(target, labels)| labels.into_iter().map(move |label| (target, label)))
            .map(|(target, label)| RefEntry {
                is_head: label.kind == RefKind::LocalBranch && label.name == head,
                name: label.name,
                kind: label.kind,
                target,
            })
            .collect::<Vec<_>>();
        refs.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(refs)
    }

    fn status(&self) -> Result<WorkingState> {
        let output = self.git(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        Ok(remote_status(&output))
    }
}

fn remote_status(output: &str) -> WorkingState {
    let mut state = WorkingState::default();
    let mut records = output.split('\0');
    while let Some(record) = records.next() {
        if record.len() < 4 { continue }
        let bytes = record.as_bytes();
        let staged = (bytes[0] as char != '?').then(|| change_kind(bytes[0] as char, bytes[1] as char)).flatten();
        let unstaged = (bytes[1] as char != '?').then(|| change_kind(bytes[1] as char, bytes[0] as char)).flatten()
            .or_else(|| (bytes[0] as char == '?' && bytes[1] as char == '?').then_some(ChangeKind::Untracked));
        let path = record[3..].to_string();
        if let Some(kind) = staged {
            state.staged.push(FileChange { path: path.clone(), kind, staged: true });
        }
        if let Some(kind) = unstaged {
            state.unstaged.push(FileChange { path: path.clone(), kind, staged: false });
        }
        // `-z` puts the source name of a rename/copy in the following record.
        if matches!(bytes[0] as char, 'R' | 'C') || matches!(bytes[1] as char, 'R' | 'C') {
            let _ = records.next();
        }
    }
    state.staged.sort_by(|a, b| a.path.cmp(&b.path));
    state.unstaged.sort_by(|a, b| a.path.cmp(&b.path));
    state
}

fn change_kind(code: char, other: char) -> Option<ChangeKind> {
    Some(match code {
        ' ' => return None,
        '?' => ChangeKind::Untracked,
        'A' => ChangeKind::Added,
        'M' => ChangeKind::Modified,
        'D' => ChangeKind::Deleted,
        'R' => ChangeKind::Renamed,
        'C' | 'T' => ChangeKind::TypeChange,
        'U' | _ if other == 'U' => ChangeKind::Conflicted,
        _ => return None,
    })
}

fn remote_commit(row: &str, labels: &HashMap<Oid, Vec<RefLabel>>, head: &str) -> Result<Commit> {
    let fields: Vec<&str> = row.split('\x1f').collect();
    anyhow::ensure!(fields.len() == 6, "remote Git returned an incomplete history row");
    let id = Oid::from_str(fields[0]).context("remote Git returned an invalid commit id")?;
    let mut refs = labels.get(&id).cloned().unwrap_or_default();
    if refs.iter().any(|label| label.kind == RefKind::LocalBranch && label.name == head) {
        refs.insert(0, RefLabel { name: head.to_string(), kind: RefKind::Head });
    }
    Ok(Commit {
        id,
        short_id: fields[1].to_string(),
        author: fields[2].to_string(),
        time: fields[3].parse().context("remote Git returned an invalid commit time")?,
        parents: fields[4].split_whitespace().filter_map(|id| Oid::from_str(id).ok()).collect(),
        summary: fields[5].trim_end_matches('\n').to_string(),
        refs,
    })
}

fn label(reference: &str) -> Option<(RefKind, String)> {
    reference.strip_prefix("refs/heads/").map(|name| (RefKind::LocalBranch, name.to_string()))
        .or_else(|| reference.strip_prefix("refs/remotes/").map(|name| (RefKind::RemoteBranch, name.to_string())))
        .or_else(|| reference.strip_prefix("refs/tags/").map(|name| (RefKind::Tag, name.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_names_keep_their_kind() {
        assert!(matches!(label("refs/heads/main"), Some((RefKind::LocalBranch, _))));
        assert!(matches!(label("refs/remotes/origin/main"), Some((RefKind::RemoteBranch, _))));
        assert!(matches!(label("refs/tags/v1"), Some((RefKind::Tag, _))));
    }

    #[test]
    fn status_keeps_staged_unstaged_and_renamed_files_apart() {
        let state = remote_status("M  staged.rs\0 M unstaged.rs\0RM renamed.rs\0before.rs\0?? new file\0");
        assert_eq!(state.staged.len(), 2);
        assert_eq!(state.unstaged.len(), 3);
        assert!(state.unstaged.iter().any(|change| change.path == "renamed.rs"));
        assert!(state.unstaged.iter().any(|change| change.path == "unstaged.rs"));
        assert!(state.unstaged.iter().any(|change| change.path == "new file"));
    }
}
