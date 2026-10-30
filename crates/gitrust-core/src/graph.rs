//! Cálculo del trazado del grafo de commits.
//!
//! El algoritmo asigna a cada commit un carril (una columna vertical) y produce
//! las líneas que conectan cada fila con la siguiente. Recorre los commits de
//! arriba abajo manteniendo, para cada carril activo, qué commit espera
//! encontrar más abajo.
//!
//! Cuando llega un commit:
//!
//! 1. Los carriles que lo esperaban convergen en él. El commit ocupa el carril
//!    libre más a la izquierda de ese grupo, y las líneas de la banda anterior
//!    se corrigen para que terminen ahí.
//! 2. Su primer padre continúa en ese mismo carril, lo que mantiene el color de
//!    una rama estable a lo largo de su historia.
//! 3. Cada padre adicional (una fusión) abre un carril nuevo a su derecha.

use git2::Oid;

use crate::model::{Commit, Graph, Link, Row};

/// Reserva un carril, reutilizando un hueco libre antes de ensanchar el grafo.
fn alloc_lane(lanes: &mut Vec<Option<Oid>>, colors: &mut Vec<usize>, color: usize) -> usize {
    match lanes.iter().position(Option::is_none) {
        Some(index) => {
            colors[index] = color;
            index
        }
        None => {
            lanes.push(None);
            colors.push(color);
            lanes.len() - 1
        }
    }
}

/// Convierte una lista de commits en un grafo con carriles y líneas.
pub fn layout(commits: Vec<Commit>) -> Graph {
    let mut rows: Vec<Row> = Vec::with_capacity(commits.len());

    // Para cada carril, qué commit espera encontrar más abajo.
    let mut lanes: Vec<Option<Oid>> = Vec::new();
    let mut colors: Vec<usize> = Vec::new();
    let mut next_color = 0usize;
    let mut lane_count = 0usize;

    for commit in commits {
        // Carriles que esperaban justo este commit.
        let expecting: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| (slot.as_ref() == Some(&commit.id)).then_some(index))
            .collect();

        // El commit va al primero de ellos; si no lo esperaba nadie es la punta
        // de una rama y estrena carril.
        let lane = match expecting.first() {
            Some(&index) => index,
            None => {
                let color = next_color;
                next_color += 1;
                alloc_lane(&mut lanes, &mut colors, color)
            }
        };
        let color = colors[lane];

        // Las líneas de la banda de arriba se dibujaron apuntando a su propio
        // carril porque aún no sabíamos dónde caería este commit. Ahora sí.
        if let Some(previous) = rows.last_mut() {
            for link in previous.links.iter_mut() {
                if expecting.contains(&link.to) {
                    link.to = lane;
                }
            }
        }

        // Los carriles que convergieron aquí quedan libres.
        for &index in &expecting {
            lanes[index] = None;
        }

        // El primer padre hereda el carril; los demás abren uno nuevo salvo que
        // otro carril ya los estuviera esperando.
        let mut touched = vec![lane];
        let mut parents = commit.parents.iter();
        match parents.next() {
            Some(&first) => {
                lanes[lane] = Some(first);
                colors[lane] = color;
                for &parent in parents {
                    if lanes.iter().any(|slot| slot.as_ref() == Some(&parent)) {
                        continue;
                    }
                    let branch_color = next_color;
                    next_color += 1;
                    let index = alloc_lane(&mut lanes, &mut colors, branch_color);
                    lanes[index] = Some(parent);
                    touched.push(index);
                }
            }
            // Un commit raíz cierra su carril.
            None => lanes[lane] = None,
        }

        // Con los carriles ya actualizados, las líneas hacia la fila siguiente:
        // las que este commit acaba de abrir salen de su punto, el resto bajan
        // rectas por su propio carril.
        let links = lanes
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.is_some())
            .map(|(index, _)| Link {
                from: if touched.contains(&index) { lane } else { index },
                to: index,
                color: colors[index],
            })
            .collect();

        lane_count = lane_count.max(lanes.len());
        rows.push(Row { commit, lane, color, links });
    }

    // La última fila no tiene banda inferior que dibujar.
    if let Some(last) = rows.last_mut() {
        last.links.clear();
    }

    Graph { rows, lane_count }
}

/// Igual que `layout`, pero con una fila de cambios sin confirmar encima de
/// todo, colgando de HEAD.
///
/// La fila entra en el trazado como un commit más, con HEAD por padre, así que
/// el algoritmo la coloca sola: ocupa un carril, HEAD converge en él y hereda
/// su color, y la línea que los une la dibuja el mismo código que une dos
/// commits. Si HEAD no es lo más reciente de la historia —porque uno está en
/// una rama vieja—, esa línea baja por el carril de la izquierda hasta donde
/// esté, que es justo lo que hay que ver.
///
/// Si HEAD no aparece en la lista (porque el límite la cortó antes) no se
/// añade nada: una línea que bajase hasta el final sin llegar a ningún sitio
/// diría algo falso.
pub fn layout_with_working(commits: Vec<Commit>, head: Oid) -> Graph {
    if !commits.iter().any(|commit| commit.id == head) {
        return layout(commits);
    }

    let mut all = Vec::with_capacity(commits.len() + 1);
    all.push(Commit::working(head));
    all.extend(commits);
    layout(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Commit;

    /// Oid determinista a partir de un número, para que los tests se lean bien.
    fn oid(n: u8) -> Oid {
        Oid::from_str(&format!("{n:040x}")).unwrap()
    }

    fn commit(id: u8, parents: &[u8]) -> Commit {
        Commit {
            id: oid(id),
            short_id: format!("{id:07x}"),
            summary: format!("commit {id}"),
            author: "test".to_string(),
            time: 0,
            parents: parents.iter().copied().map(oid).collect(),
            refs: Vec::new(),
        }
    }

    #[test]
    fn historia_lineal_ocupa_un_solo_carril() {
        let graph = layout(vec![commit(3, &[2]), commit(2, &[1]), commit(1, &[])]);

        assert_eq!(graph.lane_count, 1);
        assert!(graph.rows.iter().all(|row| row.lane == 0));

        // Cada fila salvo la última baja recta por el carril 0.
        for row in &graph.rows[..2] {
            assert_eq!(row.links.len(), 1);
            assert_eq!((row.links[0].from, row.links[0].to), (0, 0));
        }
        assert!(graph.rows[2].links.is_empty());
    }

    #[test]
    fn una_fusion_abre_y_cierra_un_carril() {
        // M fusiona A y B, que parten ambos de la raíz R.
        let graph = layout(vec![
            commit(4, &[3, 2]),
            commit(3, &[1]),
            commit(2, &[1]),
            commit(1, &[]),
        ]);

        assert_eq!(graph.lane_count, 2);
        let lanes: Vec<usize> = graph.rows.iter().map(|row| row.lane).collect();
        assert_eq!(lanes, vec![0, 0, 1, 0], "M y A en el carril 0, B en el 1, R de vuelta al 0");

        // La fusión sale de su punto hacia el carril nuevo.
        let merge = &graph.rows[0];
        assert!(merge.links.iter().any(|l| l.from == 0 && l.to == 1));

        // Y el carril 1 converge en la raíz, que está en el carril 0.
        let before_root = &graph.rows[2];
        assert!(before_root.links.iter().any(|l| l.from == 1 && l.to == 0));
    }

    #[test]
    fn el_primer_padre_conserva_el_color_de_la_rama() {
        let graph = layout(vec![commit(4, &[3, 2]), commit(3, &[1]), commit(2, &[1]), commit(1, &[])]);

        // M, A y R comparten carril 0, así que comparten color.
        assert_eq!(graph.rows[0].color, graph.rows[1].color);
        assert_eq!(graph.rows[1].color, graph.rows[3].color);
        // La rama fusionada estrena color.
        assert_ne!(graph.rows[2].color, graph.rows[0].color);
    }

    #[test]
    fn los_cambios_sin_confirmar_cuelgan_de_head_y_visten_su_color() {
        let graph = layout_with_working(vec![commit(2, &[1]), commit(1, &[])], oid(2));

        assert_eq!(graph.rows.len(), 3, "la fila de cambios va por delante");
        let working = &graph.rows[0];
        assert!(working.commit.is_working());
        assert_eq!(working.commit.parents, vec![oid(2)]);

        // Comparte carril y color con HEAD: son la misma rama.
        assert_eq!(working.lane, graph.rows[1].lane);
        assert_eq!(working.color, graph.rows[1].color);

        // Y una sola línea recta los une.
        assert_eq!(working.links.len(), 1);
        assert_eq!((working.links[0].from, working.links[0].to), (0, 0));
    }

    #[test]
    fn la_linea_baja_hasta_head_aunque_no_sea_lo_mas_reciente() {
        // Estamos en la rama vieja (commit 2) mientras otra sigue adelante.
        let graph = layout_with_working(
            vec![commit(4, &[3]), commit(3, &[1]), commit(2, &[1]), commit(1, &[])],
            oid(2),
        );

        let working_lane = graph.rows[0].lane;
        let head = graph
            .rows
            .iter()
            .position(|row| row.commit.id == oid(2))
            .expect("HEAD sigue en el grafo");

        // El carril de los cambios se mantiene ocupado hasta llegar a HEAD.
        for row in &graph.rows[..head] {
            assert!(
                row.links.iter().any(|link| link.to == working_lane),
                "la línea tiene que seguir bajando por su carril"
            );
        }
        assert_eq!(graph.rows[head].lane, working_lane, "y terminar en HEAD");
    }

    #[test]
    fn sin_head_en_la_historia_no_se_anade_la_fila() {
        // El límite dejó fuera al commit donde está HEAD.
        let graph = layout_with_working(vec![commit(3, &[2])], oid(1));

        assert_eq!(graph.rows.len(), 1);
        assert!(!graph.rows[0].commit.is_working());
    }
}
