use crate::database::models::PrivacyRule;

pub fn matches_privacy_rule(pattern: &str, domain: &str, url: &str) -> bool {
    if pattern.trim().eq_ignore_ascii_case("ip:non-public") {
        return crate::history::health::is_non_public_url(url);
    }
    let d_lower = domain.to_lowercase();
    let u_lower = url.to_lowercase();
    let p_lower = pattern.to_lowercase();
    if let Some(suffix) = p_lower.strip_prefix("*.") {
        d_lower == suffix || d_lower.ends_with(&format!(".{}", suffix))
    } else {
        d_lower.contains(&p_lower) || u_lower.contains(&p_lower)
    }
}

pub(super) fn escape_like_literal(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn append_rule_conditions(
    rules: &[PrivacyRule],
    domain_col: &str,
    url_col: &str,
    conditions: &mut Vec<String>,
    param_values: &mut Vec<Box<dyn rusqlite::ToSql>>,
) {
    for r in rules {
        conditions.push(format!(
            "browsory_matches_privacy_rule(?, COALESCE({domain_col}, ''), COALESCE({url_col}, '')) = 0"
        ));
        param_values.push(Box::new(r.pattern.clone()));
    }
}

/// Local views retain Private rows; every AI context excludes all enabled rules.
#[derive(Clone, Copy)]
pub(super) enum DataScope {
    Local,
    Ai,
}

pub fn build_hidden_rule_conditions(
    rules: &[PrivacyRule],
    domain_col: &str,
    url_col: &str,
    conditions: &mut Vec<String>,
    params: &mut Vec<Box<dyn rusqlite::ToSql>>,
) {
    build_rule_conditions(
        rules,
        DataScope::Local,
        domain_col,
        url_col,
        conditions,
        params,
    );
}

pub(super) fn build_rule_conditions(
    rules: &[PrivacyRule],
    scope: DataScope,
    domain_col: &str,
    url_col: &str,
    conditions: &mut Vec<String>,
    params: &mut Vec<Box<dyn rusqlite::ToSql>>,
) {
    let applicable: Vec<_> = rules
        .iter()
        .filter(|rule| {
            rule.enabled && (matches!(scope, DataScope::Ai) || rule.rule_type == "hidden")
        })
        .cloned()
        .collect();
    append_rule_conditions(&applicable, domain_col, url_col, conditions, params);
    if matches!(scope, DataScope::Ai) {
        // Local file paths and browser-internal pages are not cloud context.
        conditions.push(format!("(lower(COALESCE({url_col}, '')) LIKE 'http://%' OR lower(COALESCE({url_col}, '')) LIKE 'https://%')"));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn unicode_rules_match_before_ai_limits_and_hidden_queries() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::database::migrations::run_migrations(&mut conn).unwrap();
        for (id, path) in [(1, "ÄRZTE"), (2, "ΠΑΘΟΣ"), (3, "БАНК"), (4, "public")] {
            conn.execute(
                "INSERT INTO urls(id,url,title,domain) VALUES(?1,?2,'fixture','public.example')",
                rusqlite::params![id, format!("https://public.example/{path}")],
            )
            .unwrap();
        }
        for pattern in ["ärzte", "παθος", "банк"] {
            crate::database::repository::add_privacy_rule(&conn, pattern, "hidden").unwrap();
        }
        let rows = crate::database::repository::get_unindexed_urls(&conn, "fixture", 1).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 4);
        assert_eq!(
            crate::database::repository::get_embedding_indexing_status(&conn, "fixture")
                .unwrap()
                .total_urls,
            1
        );
    }
}
