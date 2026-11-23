use crate::ai::http::{
    read_response_body_limited, read_response_text_limited, MAX_AI_RESPONSE_BYTES,
};
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};

pub const MAX_EMBEDDING_BATCH_SIZE: usize = 100;
pub const MAX_EMBEDDING_DIMENSION: usize = 8192;
pub const MAX_EMBEDDING_TOTAL_ELEMENTS: usize = 1_000_000;
pub const MAX_EMBEDDING_RESULT_LIMIT: usize = 100;

pub fn validate_embedding_batch_size(value: Option<usize>) -> AppResult<usize> {
    let batch_size = value.unwrap_or(20);
    if batch_size == 0 || batch_size > MAX_EMBEDDING_BATCH_SIZE {
        return Err(AppError::Other(format!(
            "Embedding 批大小必须在 1 到 {} 之间",
            MAX_EMBEDDING_BATCH_SIZE
        )));
    }
    Ok(batch_size)
}

pub fn validate_embedding_result_limit(value: Option<usize>, default: usize) -> AppResult<usize> {
    let limit = value.unwrap_or(default);
    if limit == 0 || limit > MAX_EMBEDDING_RESULT_LIMIT {
        return Err(AppError::Other(format!(
            "Embedding 查询 limit 必须在 1 到 {} 之间",
            MAX_EMBEDDING_RESULT_LIMIT
        )));
    }
    Ok(limit)
}

pub fn floats_to_bytes(floats: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(floats.len() * 4);
    for &f in floats {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    bytes
}

pub fn bytes_to_floats(bytes: &[u8]) -> Vec<f32> {
    let mut floats = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let arr: [u8; 4] = chunk.try_into().unwrap_or([0, 0, 0, 0]);
        floats.push(f32::from_le_bytes(arr));
    }
    floats
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }

    let denominator = (norm_a.sqrt() * norm_b.sqrt()).max(1e-9);
    dot / denominator
}

pub fn rrf_score(fts_rank: Option<usize>, vec_rank: Option<usize>, k: f64) -> f64 {
    let mut score = 0.0;
    if let Some(r) = fts_rank {
        score += 1.0 / (k + (r + 1) as f64);
    }
    if let Some(r) = vec_rank {
        score += 1.0 / (k + (r + 1) as f64);
    }
    score
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingItem {
    embedding: Vec<f32>,
    index: usize,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingItem>,
}

pub async fn fetch_embeddings(
    base_url: &str,
    api_key: &str,
    model: &str,
    texts: &[&str],
) -> AppResult<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    if texts.len() > MAX_EMBEDDING_BATCH_SIZE {
        return Err(AppError::Other(format!(
            "Embedding 输入批次超过 {} 条上限",
            MAX_EMBEDDING_BATCH_SIZE
        )));
    }

    let trimmed_base = base_url.trim().trim_end_matches('/');
    let endpoint = if trimmed_base.ends_with("/embeddings") {
        trimmed_base.to_string()
    } else {
        format!("{}/embeddings", trimmed_base)
    };

    let client = crate::ai::http::ai_client(trimmed_base, std::time::Duration::from_secs(60))
        .map_err(|e| AppError::Other(format!("创建 HTTP 客户端失败: {}", e)))?;

    let req_body = EmbeddingRequest {
        model,
        input: texts.to_vec(),
    };

    let mut req = client.post(&endpoint).json(&req_body);
    if !api_key.trim().is_empty() {
        req = req.header("Authorization", format!("Bearer {}", api_key.trim()));
    }

    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Other(format!("向量接口请求失败: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err_text = read_response_text_limited(resp, MAX_AI_RESPONSE_BYTES)
            .await
            .unwrap_or_else(|e| e);
        return Err(AppError::Other(format!(
            "向量接口错误 (HTTP {}): {}",
            status, err_text
        )));
    }

    let body = read_response_body_limited(resp, MAX_AI_RESPONSE_BYTES)
        .await
        .map_err(AppError::Other)?;
    let parsed: EmbeddingResponse = serde_json::from_slice(&body)
        .map_err(|e| AppError::Other(format!("向量响应解析失败: {}", e)))?;
    let result = validate_embedding_items(parsed.data, texts.len())?;

    if result.len() != texts.len() {
        return Err(AppError::Other(format!(
            "返回向量数量 ({}) 与输入文本数量 ({}) 不一致",
            result.len(),
            texts.len()
        )));
    }

    Ok(result)
}

fn validate_embedding_items(
    items: Vec<EmbeddingItem>,
    expected_count: usize,
) -> AppResult<Vec<Vec<f32>>> {
    if items.len() != expected_count {
        return Err(AppError::Other(format!(
            "返回向量数量 ({}) 与输入文本数量 ({}) 不一致",
            items.len(),
            expected_count
        )));
    }

    let mut ordered: Vec<Option<Vec<f32>>> = (0..expected_count).map(|_| None).collect();
    let mut expected_dimension: Option<usize> = None;
    let mut total_elements = 0usize;

    for item in items {
        if item.index >= expected_count {
            return Err(AppError::Other(format!(
                "向量 index {} 超出输入范围 (0..{})",
                item.index, expected_count
            )));
        }
        if ordered[item.index].is_some() {
            return Err(AppError::Other(format!(
                "向量响应包含重复 index {}",
                item.index
            )));
        }

        let dimension = item.embedding.len();
        if dimension == 0 || dimension > MAX_EMBEDDING_DIMENSION {
            return Err(AppError::Other(format!(
                "向量维度 {} 无效，必须在 1 到 {} 之间",
                dimension, MAX_EMBEDDING_DIMENSION
            )));
        }
        if let Some(previous) = expected_dimension {
            if previous != dimension {
                return Err(AppError::Other(format!(
                    "向量维度不一致 ({} 与 {})",
                    previous, dimension
                )));
            }
        } else {
            expected_dimension = Some(dimension);
        }

        total_elements = total_elements
            .checked_add(dimension)
            .ok_or_else(|| AppError::Other("向量总元素数量溢出".into()))?;
        if total_elements > MAX_EMBEDDING_TOTAL_ELEMENTS {
            return Err(AppError::Other(format!(
                "向量总元素数量超过 {} 上限",
                MAX_EMBEDDING_TOTAL_ELEMENTS
            )));
        }
        if item.embedding.iter().any(|value| !value.is_finite()) {
            return Err(AppError::Other(format!(
                "向量 index {} 包含非有限数值",
                item.index
            )));
        }

        ordered[item.index] = Some(item.embedding);
    }

    ordered
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            item.ok_or_else(|| AppError::Other(format!("向量响应缺少 index {}", index)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_float_byte_roundtrip() {
        let floats = vec![0.123f32, -0.456, 1.0, 0.0, -1.0, 999.999];
        let bytes = floats_to_bytes(&floats);
        assert_eq!(bytes.len(), floats.len() * 4);
        let recovered = bytes_to_floats(&bytes);
        assert_eq!(floats.len(), recovered.len());
        for (a, b) in floats.iter().zip(recovered.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-5);

        let orthogonal = vec![0.0, 1.0, 0.0];
        let sim_ortho = cosine_similarity(&a, &orthogonal);
        assert!(sim_ortho.abs() < 1e-5);

        let opposite = vec![-1.0, 0.0, 0.0];
        let sim_opp = cosine_similarity(&a, &opposite);
        assert!((sim_opp - (-1.0)).abs() < 1e-5);
    }

    #[test]
    fn test_rrf_scoring() {
        let s1 = rrf_score(Some(0), Some(0), 60.0);
        let s2 = rrf_score(Some(0), None, 60.0);
        let s3 = rrf_score(None, Some(0), 60.0);
        let s4 = rrf_score(Some(10), Some(10), 60.0);

        assert!(s1 > s2);
        assert!(s1 > s3);
        assert!(s4 > s2);
        let s5 = rrf_score(Some(100), None, 60.0);
        assert!(s2 > s5);
    }

    #[test]
    fn rejects_duplicate_out_of_range_and_non_finite_vectors() {
        let duplicate = vec![
            EmbeddingItem {
                index: 0,
                embedding: vec![1.0],
            },
            EmbeddingItem {
                index: 0,
                embedding: vec![2.0],
            },
        ];
        assert!(validate_embedding_items(duplicate, 2).is_err());

        let out_of_range = vec![EmbeddingItem {
            index: 2,
            embedding: vec![1.0],
        }];
        assert!(validate_embedding_items(out_of_range, 1).is_err());

        let non_finite = vec![EmbeddingItem {
            index: 0,
            embedding: vec![f32::NAN],
        }];
        assert!(validate_embedding_items(non_finite, 1).is_err());
    }

    #[test]
    fn orders_valid_vectors_and_rejects_dimension_mismatch() {
        let items = vec![
            EmbeddingItem {
                index: 1,
                embedding: vec![2.0, 3.0],
            },
            EmbeddingItem {
                index: 0,
                embedding: vec![1.0, 4.0],
            },
        ];
        let ordered = validate_embedding_items(items, 2).unwrap();
        assert_eq!(ordered, vec![vec![1.0, 4.0], vec![2.0, 3.0]]);

        let mismatched = vec![
            EmbeddingItem {
                index: 0,
                embedding: vec![1.0],
            },
            EmbeddingItem {
                index: 1,
                embedding: vec![2.0, 3.0],
            },
        ];
        assert!(validate_embedding_items(mismatched, 2).is_err());
    }

    #[test]
    fn fetch_embeddings_rejects_redirects_without_forwarding_credentials() {
        for status in [302, 307, 308] {
            let target = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            target.set_nonblocking(true).unwrap();
            let destination = format!("http://{}/steal", target.local_addr().unwrap());
            let origin = crate::ai::test_http::FixtureServer::start(format!(
                "HTTP/1.1 {status} Redirect\r\nLocation: {destination}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ));
            let result = tauri::async_runtime::block_on(fetch_embeddings(
                &origin.base_url,
                "synthetic-api-key",
                "text-embedding-3-small",
                &["synthetic embedding payload"],
            ));
            assert!(result.is_err());
            assert!(
                matches!(target.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
            );
        }
    }
}
