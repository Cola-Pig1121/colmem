use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

fn local_embedding_model() -> EmbeddingModel {
    match std::env::var("COLMEM_LOCAL_EMBEDDING_MODEL")
        .unwrap_or_else(|_| "BAAI/bge-small-zh-v1.5".to_string())
        .as_str()
    {
        "BAAI/bge-large-zh-v1.5" | "bge-large-zh-v1.5" => EmbeddingModel::BGELargeZHV15,
        _ => EmbeddingModel::BGESmallZHV15,
    }
}

pub fn embed_texts(texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    let mut model = TextEmbedding::try_new(
        InitOptions::new(local_embedding_model()).with_show_download_progress(true),
    )
    .map_err(|err| err.to_string())?;
    model
        .embed(texts.to_vec(), None)
        .map_err(|err| err.to_string())
}
