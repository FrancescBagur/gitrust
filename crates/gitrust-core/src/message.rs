//! Composición del mensaje de commit a partir del título y la descripción.
//!
//! Git trata la primera línea como el asunto y espera una línea en blanco
//! antes del cuerpo. Separarlos en la interfaz evita que eso se olvide.

/// Longitud a partir de la cual el título se marca como largo.
///
/// 72 es el ancho con el que `git log` indenta sin desbordar una terminal de
/// 80 columnas. No es un límite duro: avisamos, no impedimos.
pub const SUBJECT_LIMIT: usize = 72;

/// Une título y descripción con la línea en blanco que Git espera.
pub fn compose(subject: &str, body: &str) -> String {
    let subject = subject.trim();
    let body = body.trim();

    if body.is_empty() {
        subject.to_string()
    } else {
        format!("{subject}\n\n{body}")
    }
}

/// Un trozo del cuerpo de un mensaje de commit.
///
/// Los cuerpos vienen plegados a mano a unas setenta columnas, que es lo que
/// pide una terminal pero no lo que mide un panel. Volver a plegar ese texto
/// al ancho que haya produce un renglón corto detrás de cada largo, así que
/// primero hay que deshacer los saltos de la prosa. Lo que no es prosa —una
/// lista, un trozo de código, un *trailer*— se deja intacto, porque ahí el
/// salto de línea significa algo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// Prosa corrida: se puede volver a plegar al ancho disponible.
    Prose(String),
    /// Texto que se respeta tal cual.
    Literal(String),
}

/// ¿Esta línea pierde su sentido si se la une con la siguiente?
fn is_literal(line: &str) -> bool {
    // Sangrada: casi siempre código o continuación de una lista.
    if line.starts_with(' ') || line.starts_with('\t') {
        return true;
    }

    let trimmed = line.trim_end();
    if trimmed.is_empty() {
        return false;
    }

    // Viñetas, citas, títulos y tablas.
    if let Some(first) = trimmed.chars().next() {
        if matches!(first, '-' | '*' | '+' | '•' | '>' | '#' | '|' | '=') {
            return true;
        }
    }

    // Listas numeradas: «1. algo», «2) algo».
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() {
        if let Some(after) = trimmed[digits.len()..].chars().next() {
            if matches!(after, '.' | ')') {
                return true;
            }
        }
    }

    // Una sola palabra, pero solo si parece una ruta o un identificador: la
    // última línea de un párrafo plegado a menudo es una palabra suelta, y esa
    // sí hay que volver a unirla.
    if !trimmed.contains(' ')
        && (trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("::"))
    {
        return true;
    }

    // Un trailer de Git: «Co-Authored-By: …», «Signed-off-by: …».
    if let Some((key, _)) = trimmed.split_once(": ") {
        if !key.is_empty()
            && !key.contains(' ')
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return true;
        }
    }

    false
}

/// Parte el cuerpo en bloques, uniendo los renglones de la prosa.
///
/// Un bloque entero se respeta en cuanto una sola de sus líneas lo pide: en un
/// párrafo mezclado es más seguro dejarlo como está que unir lo que no debía.
pub fn blocks(body: &str) -> Vec<Block> {
    let mut out = Vec::new();

    for chunk in body.split("\n\n") {
        let lines: Vec<&str> = chunk.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            continue;
        }

        if lines.iter().any(|line| is_literal(line)) {
            out.push(Block::Literal(
                lines
                    .iter()
                    .map(|l| l.trim_end())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ));
        } else {
            out.push(Block::Prose(
                lines
                    .iter()
                    .map(|l| l.trim())
                    .collect::<Vec<_>>()
                    .join(" "),
            ));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_descripcion_el_mensaje_es_solo_el_titulo() {
        assert_eq!(compose("feat: algo", ""), "feat: algo");
        assert_eq!(compose("  feat: algo  ", "   "), "feat: algo");
    }

    #[test]
    fn el_cuerpo_va_tras_una_linea_en_blanco() {
        assert_eq!(
            compose("feat: algo", "Por qué hacía falta."),
            "feat: algo\n\nPor qué hacía falta."
        );
    }

    #[test]
    fn los_saltos_internos_del_cuerpo_se_conservan() {
        let body = "Primer párrafo.\n\n- una viñeta\n- otra";
        let message = compose("fix: lo otro", body);

        assert!(message.starts_with("fix: lo otro\n\n"));
        assert!(message.ends_with("- otra"));
        assert_eq!(message.lines().count(), 6);
    }

    #[test]
    fn la_prosa_plegada_a_mano_se_vuelve_a_unir() {
        let body = "Compara dos periodos de la consola\ny deja el informe fuera del repo.";
        assert_eq!(
            blocks(body),
            vec![Block::Prose(
                "Compara dos periodos de la consola y deja el informe fuera del repo."
                    .to_string()
            )]
        );
    }

    #[test]
    fn una_lista_conserva_sus_saltos() {
        let body = "- añade el comando\n- documenta las credenciales";
        assert_eq!(
            blocks(body),
            vec![Block::Literal(
                "- añade el comando\n- documenta las credenciales".to_string()
            )]
        );
    }

    #[test]
    fn los_parrafos_y_los_trailers_salen_por_separado() {
        let body = "Primero una\nexplicación.\n\n- un punto\n\nCo-Authored-By: Alguien <a@b.c>";
        assert_eq!(
            blocks(body),
            vec![
                Block::Prose("Primero una explicación.".to_string()),
                Block::Literal("- un punto".to_string()),
                Block::Literal("Co-Authored-By: Alguien <a@b.c>".to_string()),
            ]
        );
    }

    #[test]
    fn un_bloque_con_una_linea_literal_se_respeta_entero() {
        // Si un párrafo mezcla prosa y código, unirlo rompería el código.
        let body = "Esto lo arregla:\n    cargo test --workspace";
        assert!(matches!(blocks(body).as_slice(), [Block::Literal(_)]));
    }

    #[test]
    fn una_ruta_suelta_no_se_une_al_parrafo_pero_una_palabra_si() {
        assert!(matches!(
            blocks("El fichero es\ncrates/core/src/lib.rs").as_slice(),
            [Block::Literal(_)]
        ));
        assert_eq!(
            blocks("Una frase que acaba\nasí."),
            vec![Block::Prose("Una frase que acaba así.".to_string())]
        );
    }

    #[test]
    fn el_cuerpo_vacio_no_da_bloques() {
        assert!(blocks("").is_empty());
        assert!(blocks("\n\n  \n").is_empty());
    }
}
