// Detection de trigger_words dans une transcription.
//
// Reference VoiceInk : Features/Modes/Models/ModeTriggerWordDetectionService.swift
// (VoiceInk 2.x : les mots declencheurs sont portes par les modes ; Parla
// les porte encore par les prompts, meme algorithme).
//
// Objectif : si la transcription contient (en prefixe ou suffixe) un des
// trigger_words d'un prompt custom, activer l'enhancement avec CE prompt
// et stripper le trigger du texte.
//
// Algorithme (aligne VoiceInk `detect(in:configurations:)`) :
//   1. Construire la liste de TOUS les candidats (prompt, trigger) de tous
//      les prompts, triggers vides ignores.
//   2. Trier par longueur de trigger decroissante, puis ordre du prompt,
//      puis ordre du trigger : le declencheur le plus long gagne, quel que
//      soit le prompt qui le porte (avant la 0.6.1 Parla parcourait les
//      prompts dans l'ordre, un "hey" du premier prompt volait le
//      "hey claude" du second).
//   3. Pour chaque candidat : stripTrailing puis stripLeading sur le reste
//      (prefixe ET suffixe) ; sinon stripLeading puis stripTrailing.
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

/// Analyse un texte et retourne le prompt dont un trigger_word est detecte
/// en prefixe ou suffixe, le declencheur le plus long de tous les prompts
/// etant essaye en premier. None si aucun match.
pub fn detect_and_strip(prompts: &[CustomPrompt], text: &str) -> Option<PromptDetectionResult> {
    struct Candidate<'a> {
        prompt: &'a CustomPrompt,
        trigger: String,
        prompt_index: usize,
        word_index: usize,
    }

    let mut candidates: Vec<Candidate<'_>> = Vec::new();
    for (prompt_index, prompt) in prompts.iter().enumerate() {
        for (word_index, word) in prompt.trigger_words.iter().enumerate() {
            let trimmed = word.trim();
            if trimmed.is_empty() {
                continue;
            }
            candidates.push(Candidate {
                prompt,
                trigger: trimmed.to_string(),
                prompt_index,
                word_index,
            });
        }
    }

    candidates.sort_by(|a, b| {
        b.trigger
            .chars()
            .count()
            .cmp(&a.trigger.chars().count())
            .then(a.prompt_index.cmp(&b.prompt_index))
            .then(a.word_index.cmp(&b.word_index))
    });

    for c in candidates {
        if let Some(processed) = detect_and_strip_single(text, &c.trigger) {
            return Some(PromptDetectionResult {
                prompt_id: c.prompt.id.clone(),
                processed_text: processed,
                trigger_word: c.trigger,
            });
        }
    }
    None
}

/// VoiceInk `detectAndStrip(from:triggerWord:)` : suffixe d'abord (puis
/// prefixe sur le reste), sinon prefixe (puis suffixe sur le reste).
fn detect_and_strip_single(text: &str, trigger: &str) -> Option<String> {
    if let Some(after) = strip_trailing_trigger_word(text, trigger) {
        return Some(strip_leading_trigger_word(&after, trigger).unwrap_or(after));
    }
    if let Some(after) = strip_leading_trigger_word(text, trigger) {
        return Some(strip_trailing_trigger_word(&after, trigger).unwrap_or(after));
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
    s.trim_start_matches(|c: char| matches!(c, ',' | '.' | '!' | '?' | ';' | ':') || c.is_whitespace())
        .to_string()
}

fn strip_trailing_punct_ws(s: &str) -> String {
    s.trim_end_matches(|c: char| matches!(c, ',' | '.' | '!' | '?' | ';' | ':') || c.is_whitespace())
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
    fn multi_prompt_only_matching_prompt_wins() {
        let prompts = vec![
            prompt("assistant", &["hey claude"]),
            prompt("email", &["mail"]),
        ];
        let r = detect_and_strip(&prompts, "mail bonjour").unwrap();
        assert_eq!(r.prompt_id, "email");
    }

    #[test]
    fn longest_trigger_wins_across_prompts() {
        // VoiceInk ModeTriggerWordDetectionService : les candidats de tous
        // les prompts sont tries par longueur. "hey claude" (2e prompt) doit
        // battre "hey" (1er prompt) meme si ce dernier matche aussi.
        let prompts = vec![
            prompt("chat", &["hey"]),
            prompt("assistant", &["hey claude"]),
        ];
        let r = detect_and_strip(&prompts, "hey claude what time is it").unwrap();
        assert_eq!(r.prompt_id, "assistant");
        assert_eq!(r.trigger_word, "hey claude");
        assert_eq!(r.processed_text, "What time is it");
    }

    #[test]
    fn equal_length_keeps_prompt_order() {
        let prompts = vec![prompt("first", &["mail"]), prompt("second", &["MAIL"])];
        let r = detect_and_strip(&prompts, "mail bonjour").unwrap();
        assert_eq!(r.prompt_id, "first");
    }

    #[test]
    fn empty_triggers_skipped() {
        let prompts = vec![
            prompt("email", &[""]),
            prompt("chat", &["hey"]),
        ];
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
