use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};

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

    let trimmed_base = base_url.trim().trim_end_matches('/');
    let endpoint = if trimmed_base.ends_with("/embeddings") {
        trimmed_base.to_string()
    } else {
        format!("{}/embeddings", trimmed_base)
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
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
        let err_text = resp.text().await.unwrap_or_default();
        return Err(AppError::Other(format!(
            "向量接口错误 (HTTP {}): {}",
            status, err_text
        )));
    }

    let mut parsed: EmbeddingResponse = resp
        .json()
        .await
        .map_err(|e| AppError::Other(format!("向量响应解析失败: {}", e)))?;

    // Sort by original index to ensure ordering matches texts
    parsed.data.sort_by_key(|item| item.index);
    let result: Vec<Vec<f32>> = parsed.data.into_iter().map(|item| item.embedding).collect();

    if result.len() != texts.len() {
        return Err(AppError::Other(format!(
            "返回向量数量 ({}) 与输入文本数量 ({}) 不一致",
            result.len(),
            texts.len()
        )));
    }

    Ok(result)
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
}
