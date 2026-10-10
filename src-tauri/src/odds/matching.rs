//! Book player names to NBA.com ids, by name plus team. Kalshi titles carry the full name and
//! the ticker carries a team code, so a team-scoped exact match covers almost everyone. A name
//! that is unique league-wide is the fallback; anything else is reported as unmatched.

use std::collections::HashMap;

use crate::models::RosterEntry;

fn fold(character: char) -> char {
    match character {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'č' | 'ć' => 'c',
        'đ' | 'ď' => 'd',
        'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => 'e',
        'ģ' | 'ğ' => 'g',
        'í' | 'ì' | 'î' | 'ï' | 'ī' => 'i',
        'ķ' => 'k',
        'ļ' | 'ł' => 'l',
        'ñ' | 'ņ' | 'ń' | 'ň' => 'n',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' => 'o',
        'ř' => 'r',
        'š' | 'ś' | 'ş' => 's',
        'ť' | 'ţ' => 't',
        'ú' | 'ù' | 'û' | 'ü' | 'ū' | 'ů' => 'u',
        'ý' | 'ÿ' => 'y',
        'ž' | 'ź' | 'ż' => 'z',
        other => other,
    }
}

/// "Luka Dončić" and "luka doncic" agree, and "Jaren Jackson Jr." drops the suffix.
pub fn normalize(name: &str) -> String {
    let folded: String = name
        .to_lowercase()
        .chars()
        .map(fold)
        .map(|character| if character.is_alphanumeric() { character } else if character == '\'' || character == '.' { '\0' } else { ' ' })
        .filter(|character| *character != '\0')
        .collect();
    folded
        .split_whitespace()
        .filter(|token| !matches!(*token, "jr" | "sr" | "ii" | "iii" | "iv" | "v"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub struct Matcher {
    by_team: HashMap<(String, String), i64>,
    by_name: HashMap<String, Vec<i64>>,
}

impl Matcher {
    pub fn new(roster: &[RosterEntry]) -> Self {
        let mut by_team = HashMap::new();
        let mut by_name: HashMap<String, Vec<i64>> = HashMap::new();
        for entry in roster {
            let name = normalize(&entry.name);
            by_team.insert((name.clone(), entry.team_abbr.to_uppercase()), entry.player_id);
            let ids = by_name.entry(name).or_default();
            if !ids.contains(&entry.player_id) {
                ids.push(entry.player_id);
            }
        }
        Self { by_team, by_name }
    }

    pub fn find(&self, name: &str, team: Option<&str>) -> Option<i64> {
        let name = normalize(name);
        if let Some(team) = team {
            if let Some(id) = self.by_team.get(&(name.clone(), team.to_uppercase())) {
                return Some(*id);
            }
        }
        match self.by_name.get(&name).map(Vec::as_slice) {
            Some([id]) => Some(*id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i64, name: &str, team: &str) -> RosterEntry {
        RosterEntry { player_id: id, name: name.into(), team_abbr: team.into() }
    }

    #[test]
    fn names_fold_accents_punctuation_and_suffixes() {
        assert_eq!(normalize("Luka Dončić"), "luka doncic");
        assert_eq!(normalize("Jaren Jackson Jr."), "jaren jackson");
        assert_eq!(normalize("De'Aaron Fox"), "deaaron fox");
        assert_eq!(normalize("Karl-Anthony Towns"), "karl anthony towns");
        assert_eq!(normalize("  Nikola   Jokić "), "nikola jokic");
    }

    #[test]
    fn team_scoped_match_then_unique_name_then_none() {
        let roster = [
            entry(1628369, "Jayson Tatum", "BOS"),
            entry(202331, "Paul George", "PHI"),
            entry(1, "Jalen Williams", "OKC"),
            entry(2, "Jalen Williams", "DEN"),
        ];
        let matcher = Matcher::new(&roster);
        assert_eq!(matcher.find("Jayson Tatum", Some("BOS")), Some(1628369));
        // The book's team disagrees with the stored roster; the name is unique, so it still matches.
        assert_eq!(matcher.find("Paul George", Some("BOS")), Some(202331));
        assert_eq!(matcher.find("Jalen Williams", Some("OKC")), Some(1));
        assert_eq!(matcher.find("Jalen Williams", Some("LAL")), None, "two players, wrong team");
        assert_eq!(matcher.find("Nobody Here", Some("BOS")), None);
    }
}
