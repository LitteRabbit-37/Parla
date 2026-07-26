// Detection de trigger_words dans une transcription.
//
// Reference VoiceInk : VoiceInk/Services/PromptDetectionService.swift.
//
// Objectif : si la transcription contient (en prefixe ou suffixe) un des
// trigger_words d'un prompt custom, activer l'enhancement avec CE prompt
// et stripper le trigger du texte.
//
// Algorithme (aligne VoiceInk detectAndStripTriggerWord L153) :
//   1. Filtrer les triggers vides, trier par longueur decroissante pour
//      matcher le plus specifique.
//   2. Tenter d'abord stripTrailing puis stripLeading. Si trailing match,
//      tenter leading sur le reste (cas prefix ET suffix).
//   3. Si trailing echoue pour un trigger, passer au leading dans une
//      seconde passe.
//   4. Matching insensible a la casse, exige frontiere de mot (char
//      adjacent pas lettre/chiffre). Nettoyage ponctuation entourante
//      et capitalisation de la premiere lettre restante.

use crate::enhancement::prompts::CustomPrompt;

#[derive(Debug, Clone)]
pub struct PromptDetectionResult {
    pub prompt_id: String,
    pub processed_text: String,
    pub trigger_word: String,
}

/// Analyse un texte et retourne le premier prompt dont un trigger_word
/// est detecte en prefixe ou suffixe. None si aucun match.
pub fn detect_and_strip(prompts: &[CustomPrompt], text: &str) -> Option<PromptDetectionResult> {
    for prompt in prompts {
        if prompt.trigger_words.is_empty() {
            continue;
        }
        if let Some((word, processed)) = detect_and_strip_trigger_word(text, &prompt.trigger_words)
        {
            return Some(PromptDetectionResult {
                prompt_id: prompt.id.clone(),
                processed_text: processed,
                trigger_word: word,
            });
        }
    }
    None
}

fn detect_and_strip_trigger_word(text: &str, trigger_words: &[String]) -> Option<(String, String)> {
    let mut trimmed: Vec<String> = trigger_words
        .iter()
        .map(|w| w.trim().to_string())
        .filter(|w| !w.is_empty())
        .collect();
    // Tri par longueur decroissante (plus specifique d'abord).
    trimmed.sort_by_key(|b| std::cmp::Reverse(b.chars().count()));

    // Premiere passe : trailing prioritaire (VoiceInk L160).
    for trigger in &trimmed {
        if let Some(after_trailing) = strip_trailing_trigger_word(text, trigger) {
            if let Some(after_both) = strip_leading_trigger_word(&after_trailing, trigger) {
                return Some((trigger.clone(), after_both));
            }
            return Some((trigger.clone(), after_trailing));
        }
    }
    // Seconde passe : leading uniquement (VoiceInk L169).
    for trigger in &trimmed {
        if let Some(after_leading) = strip_leading_trigger_word(text, trigger) {
            if let Some(after_both) = strip_trailing_trigger_word(&after_leading, trigger) {
                return Some((trigger.clone(), after_both));
            }
            return Some((trigger.clone(), after_leading));
        }
    }
    None
}

/// Strip leading trigger. Retourne Some(text_sans_trigger) si match, None sinon.
/// Requiert frontiere de mot (char suivant pas lettre/chiffre) et nettoie
/// ponctuation + whitespace, capitalise premiere lettre.
fn strip_leading_trigger_word(text: &str, trigger: &str) -> Option<String> {
    let trimmed = text.trim();
    let lower_text = trimmed.to_lowercase();
    let lower_trigger = trigger.to_lowercase();

    if !lower_text.starts_with(&lower_trigger) {
        return None;
    }

    // Verifier frontiere de mot. On compte les chars du trigger dans le texte
    // original pour eviter les bugs UTF-8.
    let trigger_char_len = trigger.chars().count();
    let trimmed_chars: Vec<char> = trimmed.chars().collect();
    if trimmed_chars.len() > trigger_char_len {
        let boundary_char = trimmed_chars[trigger_char_len];
        if boundary_char.is_alphanumeric() {
            return None;
        }
    }

    // Reconstruit la suite en bytes a partir du char offset trigger_char_len.
    let byte_offset: usize = trimmed
        .char_indices()
        .nth(trigger_char_len)
        .map(|(i, _)| i)
        .unwrap_or(trimmed.len());
    let remaining_raw = &trimmed[byte_offset..];

    let cleaned = strip_leading_punct_ws(remaining_raw).trim().to_string();
    Some(capitalize_first(&cleaned))
}

/// Strip trailing trigger. Analogue a leading mais cote suffixe.
/// VoiceInk enleve d'abord la ponctuation trailing avant de tester le suffix
/// (ElevenLabs tends to add a period to end).
fn strip_trailing_trigger_word(text: &str, trigger: &str) -> Option<String> {
    let mut trimmed = text.trim().to_string();

    // Strip ponctuation trailing avant de matcher.
    while let Some(c) = trimmed.chars().last() {
        if matches!(c, ',' | '.' | '!' | '?' | ';' | ':') {
            trimmed.pop();
        } else {
            break;
        }
    }

    let lower_text = trimmed.to_lowercase();
    let lower_trigger = trigger.trim().to_lowercase();

    if !lower_text.ends_with(&lower_trigger) {
        return None;
    }

    // Verifier frontiere de mot. char precedant le trigger ne doit pas etre
    // alphanumerique.
    let trigger_char_len = trigger.chars().count();
    let trimmed_chars: Vec<char> = trimmed.chars().collect();
    let trigger_start_in_chars = trimmed_chars.len().saturating_sub(trigger_char_len);
    if trigger_start_in_chars > 0 {
        let boundary_char = trimmed_chars[trigger_start_in_chars - 1];
        if boundary_char.is_alphanumeric() {
            return None;
        }
    }

    let byte_offset: usize = trimmed
        .char_indices()
        .nth(trigger_start_in_chars)
        .map(|(i, _)| i)
        .unwrap_or(trimmed.len());
    let remaining_raw = &trimmed[..byte_offset];

    let cleaned = strip_trailing_punct_ws(remaining_raw).trim().to_string();
    Some(capitalize_first(&cleaned))
}

fn strip_leading_punct_ws(s: &str) -> String {
    s.trim_start_matches(|c: char| {
        matches!(c, ',' | '.' | '!' | '?' | ';' | ':') || c.is_whitespace()
    })
    .to_string()
}

fn strip_trailing_punct_ws(s: &str) -> String {
    s.trim_end_matches(|c: char| {
        matches!(c, ',' | '.' | '!' | '?' | ';' | ':') || c.is_whitespace()
    })
    .to_string()
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(id: &str, triggers: &[&str]) -> CustomPrompt {
        CustomPrompt {
            id: id.into(),
            title: "test".into(),
            prompt_text: "".into(),
            icon: "".into(),
            description: None,
            is_predefined: false,
            trigger_words: triggers.iter().map(|s| s.to_string()).collect(),
            use_system_instructions: false,
        }
    }

    #[test]
    fn leading_exact() {
        let prompts = vec![prompt("email", &["mail"])];
        let r = detect_and_strip(&prompts, "mail bonjour jean").unwrap();
        assert_eq!(r.prompt_id, "email");
        assert_eq!(r.trigger_word, "mail");
        assert_eq!(r.processed_text, "Bonjour jean");
    }

    #[test]
    fn trailing_exact() {
        let prompts = vec![prompt("email", &["mail"])];
        let r = detect_and_strip(&prompts, "bonjour jean mail").unwrap();
        assert_eq!(r.processed_text, "Bonjour jean");
    }

    #[test]
    fn case_insensitive() {
        let prompts = vec![prompt("email", &["Mail"])];
        let r = detect_and_strip(&prompts, "MAIL bonjour").unwrap();
        assert_eq!(r.trigger_word, "Mail");
    }

    #[test]
    fn trailing_with_punct() {
        let prompts = vec![prompt("email", &["mail"])];
        // ElevenLabs ajoute souvent un point final.
        let r = detect_and_strip(&prompts, "bonjour jean mail.").unwrap();
        assert_eq!(r.processed_text, "Bonjour jean");
    }

    #[test]
    fn no_match_when_substring() {
        // "mail" ne doit pas matcher dans "email" car boundary alphanumerique.
        let prompts = vec![prompt("email", &["mail"])];
        assert!(detect_and_strip(&prompts, "email bonjour").is_none());
        assert!(detect_and_strip(&prompts, "bonjour email").is_none());
    }

    #[test]
    fn longer_trigger_first() {
        // "code review" doit matcher avant "code" si les deux sont dans les triggers.
        let prompts = vec![prompt("review", &["code", "code review"])];
        let r = detect_and_strip(&prompts, "code review regarde ce fichier").unwrap();
        assert_eq!(r.trigger_word, "code review");
        assert_eq!(r.processed_text, "Regarde ce fichier");
    }

    #[test]
    fn multi_prompt_first_match_wins() {
        let prompts = vec![
            prompt("assistant", &["hey claude"]),
            prompt("email", &["mail"]),
        ];
        let r = detect_and_strip(&prompts, "mail bonjour").unwrap();
        assert_eq!(r.prompt_id, "email");
    }

    #[test]
    fn empty_triggers_skipped() {
        let prompts = vec![prompt("email", &[""]), prompt("chat", &["hey"])];
        let r = detect_and_strip(&prompts, "hey bonjour").unwrap();
        assert_eq!(r.prompt_id, "chat");
    }

    #[test]
    fn no_match_returns_none() {
        let prompts = vec![prompt("email", &["mail"])];
        assert!(detect_and_strip(&prompts, "bonjour jean comment vas-tu").is_none());
    }

    #[test]
    fn leading_with_comma() {
        let prompts = vec![prompt("email", &["mail"])];
        let r = detect_and_strip(&prompts, "mail, bonjour jean").unwrap();
        assert_eq!(r.processed_text, "Bonjour jean");
    }

    #[test]
    fn prefix_and_suffix_both_stripped() {
        let prompts = vec![prompt("review", &["review"])];
        let r = detect_and_strip(&prompts, "review regarde ce fichier review").unwrap();
        assert_eq!(r.processed_text, "Regarde ce fichier");
    }
}
