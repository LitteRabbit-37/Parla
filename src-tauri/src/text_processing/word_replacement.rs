// Service d'application des remplacements de mots sur le texte transcrit.
//
// Reference VoiceInk : Features/Dictionary/Workflows/WordReplacementService.swift
// - CSV split des variantes originales
// - Regex case-insensitive avec lookarounds : un "caractere de mot" est une
//   lettre / marque / chiffre Unicode (\p{L}\p{M}\p{N}), sauf les scripts
//   sans espaces (Han, Hiragana, Katakana, Hangul, Thai) pour qu'un trigger
//   latin colle a du CJK matche quand meme (commit 491f581).
// - Fallback substring (case-insensitive) si l'original contient un scalaire
//   dans les plages Hiragana/Katakana/CJK/Hangul/Thai

use fancy_regex::Regex as FancyRegex;

use crate::db::word_replacement::WordReplacement;

/// Applique en cascade toutes les regles enabled sur `text`.
/// Tri longest-first : evite qu'une regle courte qui matche un sous-fragment
/// d'une regle longue ne casse cette derniere (ex "good" devant "good morning").
/// Cf VoiceInk WordReplacementService.swift commit 620a843.
pub fn apply(text: &str, rules: &[WordReplacement]) -> String {
    let mut current = text.to_string();

    let mut sorted_rules: Vec<&WordReplacement> =
        rules.iter().filter(|r| r.is_enabled).collect();
    sorted_rules.sort_by(|a, b| b.original_text.len().cmp(&a.original_text.len()));

    for rule in sorted_rules {
        let mut variants: Vec<&str> = rule
            .original_text
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        variants.sort_by(|a, b| b.len().cmp(&a.len()));

        for original in variants {
            if original.is_empty() {
                continue;
            }
            if uses_word_boundaries(original) {
                current = replace_with_boundaries(&current, original, &rule.replacement_text);
            } else {
                current = replace_substring_ci(&current, original, &rule.replacement_text);
            }
        }
    }

    current
}

/// Caractere de mot au sens VoiceInk : lettre, marque combinante ou chiffre
/// Unicode, sauf les scripts sans espaces (via Script_Extensions pour que les
/// marques partagees comme le prolongateur U+30FC restent exemptees).
/// La classe ASCII [a-zA-Z0-9] utilisee avant la 0.6.1 laissait une regle
/// "ERN" matcher dans "vergrößern" (ö n'etait pas un caractere de mot).
const WORD_CHAR: &str =
    r"[\p{L}\p{M}\p{N}--[\p{scx=Han}\p{scx=Hiragana}\p{scx=Katakana}\p{scx=Hangul}\p{scx=Thai}]]";

fn replace_with_boundaries(haystack: &str, needle: &str, replacement: &str) -> String {
    // Lookarounds plutot que \b : la ponctuation devient frontiere (regle
    // "hello" matche "hello!") et "_" n'est plus traite comme word char.
    // Cf VoiceInk WordReplacementService.swift commits 620a843 + 491f581.
    // fancy-regex requis pour les lookarounds (le crate regex ne les
    // supporte pas).
    let escaped = fancy_regex::escape(needle);
    let pattern = format!(r"(?i)(?<!{WORD_CHAR}){escaped}(?!{WORD_CHAR})");
    match FancyRegex::new(&pattern) {
        Ok(re) => re.replace_all(haystack, replacement).into_owned(),
        Err(_) => replace_substring_ci(haystack, needle, replacement),
    }
}

/// Remplace toutes les occurrences case-insensitive de `needle` par
/// `replacement`. Implementation manuelle pour eviter d'instancier un regex.
fn replace_substring_ci(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return haystack.to_string();
    }
    let hay_lower = haystack.to_lowercase();
    let needle_lower = needle.to_lowercase();
    let mut out = String::with_capacity(haystack.len());
    let mut cursor = 0usize;
    while let Some(rel) = hay_lower[cursor..].find(&needle_lower) {
        let start = cursor + rel;
        // Attention aux boundaries UTF-8 : `to_lowercase` peut modifier les
        // longueurs de caracteres. On verifie que start et start+needle.len()
        // sont bien des frontieres char_boundary dans l'original.
        let end = start + needle.len();
        if !haystack.is_char_boundary(start) || !haystack.is_char_boundary(end.min(haystack.len())) {
            // Fallback prudent : skip.
            cursor = start + needle.len().max(1);
            continue;
        }
        out.push_str(&haystack[cursor..start]);
        out.push_str(replacement);
        cursor = end;
    }
    out.push_str(&haystack[cursor..]);
    out
}

/// Reproduit VoiceInk WordReplacementService.usesWordBoundaries (L56-75).
/// Si un scalaire de `original` tombe dans une plage non-spacee, retourne
/// false pour declencher le fallback substring.
pub fn uses_word_boundaries(original: &str) -> bool {
    for ch in original.chars() {
        let code = ch as u32;
        let in_non_spaced = (0x3040..=0x309F).contains(&code) // Hiragana
            || (0x30A0..=0x30FF).contains(&code)              // Katakana
            || (0x4E00..=0x9FFF).contains(&code)              // CJK Unified
            || (0xAC00..=0xD7AF).contains(&code)              // Hangul Syllables
            || (0x0E00..=0x0E7F).contains(&code); // Thai
        if in_non_spaced {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn rule(id: &str, original: &str, replacement: &str) -> WordReplacement {
        WordReplacement {
            id: id.into(),
            original_text: original.into(),
            replacement_text: replacement.into(),
            date_added: Utc::now(),
            is_enabled: true,
        }
    }

    #[test]
    fn boundary_replacement_ci() {
        let rules = vec![rule("1", "docker", "Docker")];
        let out = apply("I use docker and Docker daily, docked too.", &rules);
        assert_eq!(out, "I use Docker and Docker daily, docked too.");
    }

    #[test]
    fn csv_variants() {
        let rules = vec![rule("1", "nyc, NY, new york", "New York City")];
        let out = apply("Living in NYC and NY, also new york.", &rules);
        assert!(out.contains("New York City"));
        assert!(!out.contains("NYC"));
    }

    #[test]
    fn cjk_falls_back_to_substring() {
        // Japonais : pas de word boundaries naturelles.
        let rules = vec![rule("1", "東京", "Tokyo")];
        let out = apply("私は東京が好き。東京タワー。", &rules);
        assert!(out.contains("Tokyo"));
        assert!(!out.contains("東京"));
    }

    #[test]
    fn disabled_rule_ignored() {
        let mut r = rule("1", "hello", "hi");
        r.is_enabled = false;
        let out = apply("hello world", &[r]);
        assert_eq!(out, "hello world");
    }

    #[test]
    fn uses_word_boundaries_detects_cjk() {
        assert!(!uses_word_boundaries("東京"));
        assert!(!uses_word_boundaries("안녕하세요"));
        assert!(!uses_word_boundaries("สวัสดี"));
        assert!(uses_word_boundaries("hello"));
        assert!(uses_word_boundaries("docker, k8s"));
    }

    #[test]
    fn longest_first_overlapping_rules() {
        // Sans tri longest-first, "good" appliquee en premier ecraserait "good"
        // dans "good morning" et la regle "good morning" ne matcherait plus.
        let rules = vec![
            rule("1", "good", "GOOD"),
            rule("2", "good morning", "Hello"),
        ];
        let out = apply("good morning everyone", &rules);
        assert_eq!(out, "Hello everyone");
    }

    #[test]
    fn longest_first_overlapping_variants() {
        // Meme bug a l'echelle d'une seule regle CSV : "new" doit etre tente
        // apres "new york" pour ne pas casser le match plus specifique.
        let rules = vec![rule("1", "new, new york", "AAA")];
        let out = apply("new york", &rules);
        assert_eq!(out, "AAA");
    }

    #[test]
    fn punctuation_acts_as_boundary() {
        // Les lookarounds permettent de matcher "hello" suivi/precede de
        // ponctuation (que \b ASCII gerait deja, mais on fixe le comportement).
        let rules = vec![rule("1", "hello", "Hi")];
        let out = apply("hello! Hello, world.", &rules);
        assert_eq!(out, "Hi! Hi, world.");
    }

    #[test]
    fn unicode_letters_are_word_chars() {
        // VoiceInk 491f581 : "ERN" ne doit pas matcher dans "vergrößern"
        // (ö est une lettre), ni "cafe" dans "café" (e + accent) ni "ana"
        // dans "mañana".
        let rules = vec![rule("1", "ern", "EAN"), rule("2", "cafe", "coffee")];
        let out = apply("vergrößern ern café cafe mañana", &rules);
        assert_eq!(out, "vergrößern EAN café coffee mañana");
    }

    #[test]
    fn latin_trigger_flush_against_cjk_still_matches() {
        // Les scripts sans espaces sont exemptes : un trigger latin colle a
        // du japonais / chinois / coreen / thai matche quand meme.
        let rules = vec![rule("1", "hello", "Hi")];
        let out = apply("日本語hello ハローhello 안녕hello สวัสดีhello", &rules);
        assert_eq!(out, "日本語Hi ハローHi 안녕Hi สวัสดีHi");
    }

    #[test]
    fn underscore_is_not_word_char() {
        // \b traite "_" comme word, donc "\bhello\b" ne matchait pas "_hello_".
        // Lookarounds [a-zA-Z0-9] excluent "_" et le match passe.
        let rules = vec![rule("1", "hello", "Hi")];
        let out = apply("_hello_ and __hello__", &rules);
        assert_eq!(out, "_Hi_ and __Hi__");
    }
}
