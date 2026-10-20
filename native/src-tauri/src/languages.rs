use serde::Serialize;

#[derive(Serialize)]
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
}

/// The bundled engine is the authority, so UI options never drift from its table.
pub fn available() -> Vec<Language> {
    let mut result: Vec<_> = (0..=whisper_rs::get_lang_max_id())
        .filter_map(|id| {
            Some(Language {
                code: whisper_rs::get_lang_str(id)?,
                name: whisper_rs::get_lang_str_full(id)?,
            })
        })
        .collect();
    result.sort_by_key(|language| language.name);
    result
}

pub fn validate(language: &str, model_name: &str) -> Result<(), String> {
    if language != "auto" && !available().iter().any(|item| item.code == language) {
        return Err("Unsupported language. Choose a language from the list.".into());
    }
    if model_name.contains("English only") && !["auto", "en"].contains(&language) {
        return Err(
            "This model only supports English. Install a multilingual model for this language."
                .into(),
        );
    }
    if language == "yue" && !model_name.contains("Large V3") {
        return Err("Cantonese requires a Large V3 or Large V3 Turbo model.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_engine_languages_and_rejects_incompatible_choices() {
        let languages = available();
        assert!(languages.len() >= 99);
        for code in ["uk", "de", "ja", "ar", "en"] {
            assert!(languages.iter().any(|language| language.code == code));
            assert!(validate(code, "Whisper Small · multilingual").is_ok());
        }
        for code in ["", "english", "invalid", "en\0"] {
            assert!(validate(code, "Whisper Small · multilingual").is_err());
        }
        assert!(validate("uk", "Whisper Tiny · English only").is_err());
        assert!(validate("auto", "Whisper Tiny · English only").is_ok());
        assert!(validate("yue", "Whisper Small · multilingual").is_err());
        assert!(validate("yue", "Whisper Large V3 · multilingual").is_ok());
    }
}
