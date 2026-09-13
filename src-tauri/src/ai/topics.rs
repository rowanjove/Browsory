use crate::error::AppResult;
use rusqlite::{params, Connection};

pub struct DefaultTopicDef {
    pub name: &'static str,
    pub category: &'static str,
    pub icon: &'static str,
    pub color: &'static str,
    pub domain_keywords: &'static [&'static str],
    pub title_keywords: &'static [&'static str],
}

pub const DEFAULT_TOPICS: &[DefaultTopicDef] = &[
    DefaultTopicDef {
        name: "系统与底层工程",
        category: "技术研发",
        icon: "Cpu",
        color: "#f59e0b",
        domain_keywords: &["crates.io", "rust-lang.org", "kernel.org", "golang.org"],
        title_keywords: &[
            "rust", "tokio", "golang", "c++", "linux", "kernel", "thread", "memory", "async",
            "compiler", "并发", "内核",
        ],
    },
    DefaultTopicDef {
        name: "AI与大模型技术",
        category: "人工智能",
        icon: "Sparkles",
        color: "#8b5cf6",
        domain_keywords: &[
            "huggingface.co",
            "openai.com",
            "anthropic.com",
            "deepseek.com",
        ],
        title_keywords: &[
            "ai",
            "llm",
            "gpt",
            "claude",
            "transformer",
            "embedding",
            "agent",
            "prompt",
            "pytorch",
            "模型",
            "提示词",
            "大语言模型",
        ],
    },
    DefaultTopicDef {
        name: "前端与交互开发",
        category: "技术研发",
        icon: "Layout",
        color: "#3b82f6",
        domain_keywords: &[
            "react.dev",
            "npmjs.com",
            "developer.mozilla.org",
            "tailwindcss.com",
        ],
        title_keywords: &[
            "react",
            "typescript",
            "javascript",
            "vue",
            "tailwind",
            "vite",
            "css",
            "html",
            "next.js",
            "组件",
            "渲染",
            "前端",
        ],
    },
    DefaultTopicDef {
        name: "数据库与存储",
        category: "基础设施",
        icon: "Database",
        color: "#10b981",
        domain_keywords: &["sqlite.org", "postgresql.org", "redis.io", "mysql.com"],
        title_keywords: &[
            "sql",
            "sqlite",
            "postgres",
            "redis",
            "database",
            "query",
            "wal",
            "btree",
            "事务",
            "索引",
            "数据库",
        ],
    },
    DefaultTopicDef {
        name: "开源社区与代码协同",
        category: "开发工具",
        icon: "GitBranch",
        color: "#6366f1",
        domain_keywords: &["github.com", "gitlab.com", "gitee.com", "stackoverflow.com"],
        title_keywords: &[
            "github",
            "repo",
            "commit",
            "pull request",
            "issue",
            "branch",
            "git",
            "开源",
            "代码",
            "仓库",
        ],
    },
    DefaultTopicDef {
        name: "知识沉淀与深度阅读",
        category: "认知学习",
        icon: "BookOpen",
        color: "#06b6d4",
        domain_keywords: &[
            "zhihu.com",
            "notion.so",
            "wikipedia.org",
            "medium.com",
            "sspai.com",
        ],
        title_keywords: &[
            "知乎", "百科", "notion", "思考", "笔记", "总结", "专栏", "教程", "阅读", "文档",
        ],
    },
    DefaultTopicDef {
        name: "资讯商业与行业动向",
        category: "行业资讯",
        icon: "TrendingUp",
        color: "#ec4899",
        domain_keywords: &["v2ex.com", "news.ycombinator.com", "36kr.com", "huxiu.com"],
        title_keywords: &[
            "科技", "商业", "创业", "资讯", "新闻", "财报", "融资", "行业", "趋势", "startup",
        ],
    },
    DefaultTopicDef {
        name: "音视频与数字生活",
        category: "生活娱乐",
        icon: "PlaySquare",
        color: "#ef4444",
        domain_keywords: &["bilibili.com", "youtube.com", "spotify.com", "netflix.com"],
        title_keywords: &[
            "bilibili",
            "哔哩哔哩",
            "视频",
            "音乐",
            "游戏",
            "电影",
            "直播",
            "动画",
            "番剧",
        ],
    },
];

pub fn ensure_default_topics(conn: &Connection) -> AppResult<()> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut stmt = conn.prepare_cached(
        "INSERT OR IGNORE INTO topics (name, category, icon, color, created_at) VALUES (?1, ?2, ?3, ?4, ?5)"
    )?;

    for def in DEFAULT_TOPICS {
        stmt.execute(params![def.name, def.category, def.icon, def.color, now])?;
    }

    Ok(())
}

pub fn classify_url(title: &str, domain: &str) -> Vec<(&'static str, f64)> {
    let title_lower = title.to_lowercase();
    let domain_lower = domain.to_lowercase();
    let mut matches = Vec::new();

    for def in DEFAULT_TOPICS {
        let mut score = 0.0f64;

        for &dk in def.domain_keywords {
            if domain_lower.contains(dk) {
                score += 1.5;
            }
        }

        for &tk in def.title_keywords {
            if title_lower.contains(tk) {
                score += 1.0;
            }
        }

        if score > 0.0 {
            matches.push((def.name, score.min(2.0) / 2.0));
        }
    }

    matches
}

pub fn auto_tag_unclassified_urls(conn: &Connection, limit: usize) -> AppResult<usize> {
    ensure_default_topics(conn)?;

    let active_rules = crate::database::repository::get_active_privacy_rules(conn)?;
    let hidden_rules: Vec<_> = active_rules
        .into_iter()
        .filter(|r| r.rule_type == "hidden")
        .collect();

    let mut stmt = conn.prepare(
        r#"
        SELECT u.id, u.url, u.title, u.domain
        FROM urls u
        LEFT JOIN url_topics ut ON u.id = ut.url_id
        WHERE ut.url_id IS NULL
        LIMIT ?1
        "#,
    )?;

    let urls: Vec<(i64, String, String, String)> = stmt
        .query_map([limit], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            ))
        })?
        .flatten()
        .collect();

    let mut tagged_count = 0;
    let mut insert_topic_stmt = conn.prepare_cached(
        r#"
        INSERT OR IGNORE INTO url_topics (url_id, topic_id, confidence)
        SELECT ?1, id, ?2 FROM topics WHERE name = ?3
        "#,
    )?;

    for (url_id, url, title, domain) in urls {
        if hidden_rules
            .iter()
            .any(|r| crate::database::repository::matches_privacy_rule(&r.pattern, &domain, &url))
        {
            continue;
        }
        let classifications = classify_url(&title, &domain);
        for (topic_name, confidence) in classifications {
            insert_topic_stmt.execute(params![url_id, confidence, topic_name])?;
            tagged_count += 1;
        }
    }

    Ok(tagged_count)
}
