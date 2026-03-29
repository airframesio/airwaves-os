use axum::extract::{Path, State};
use axum::Json;

use crate::domain::FeedConfig;
use crate::ports::ConfigPort;
use crate::{AppError, AppState};

/// List all configured feeds
pub async fn list_feeds(
    State(state): State<AppState>,
) -> Result<Json<Vec<FeedConfig>>, AppError> {
    let config = state.config.read_config().await?;
    let feeds: Vec<FeedConfig> = config
        .aggregators
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(_, v)| serde_json::from_value::<FeedConfig>(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    Ok(Json(feeds))
}

/// Add or update a feed
pub async fn upsert_feed(
    State(state): State<AppState>,
    Json(feed): Json<FeedConfig>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut config = state.config.read_config().await?;

    let aggregators = config.aggregators.as_object_mut().ok_or_else(|| {
        // Initialize as empty object if not already
        AppError::Internal("aggregators is not an object".to_string())
    });

    match aggregators {
        Ok(obj) => {
            obj.insert(feed.id.clone(), serde_json::to_value(&feed).unwrap());
        }
        Err(_) => {
            let mut obj = serde_json::Map::new();
            obj.insert(feed.id.clone(), serde_json::to_value(&feed).unwrap());
            config.aggregators = serde_json::Value::Object(obj);
        }
    }

    state.config.write_config(&config).await?;
    Ok(Json(serde_json::json!({"status": "saved", "id": feed.id})))
}

/// Delete a feed
pub async fn delete_feed(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut config = state.config.read_config().await?;

    if let Some(obj) = config.aggregators.as_object_mut() {
        obj.remove(&id);
    }

    state.config.write_config(&config).await?;
    Ok(Json(serde_json::json!({"status": "deleted", "id": id})))
}
