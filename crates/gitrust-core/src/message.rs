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
}
