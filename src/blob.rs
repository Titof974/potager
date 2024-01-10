use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{SharedState, DigestPayload, RequestChunkUpload, ip, RequestErrors, RequestError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Blobs {
    pub digest: String,
    pub data: Vec<u8>,
}

pub async fn get_blob(
    Path((_name, digest)): Path<(String, String)>,
    State(state): State<SharedState>,
) -> Response {
    let resp = Response::builder();
    let registry = &state.read().unwrap().registry;
    let blob = registry.blobs.iter().find(|x| x.digest == digest);
    if blob.is_none() {
        resp.status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap()
    } else {
        let data = blob.unwrap().data.to_vec();
        resp.status(StatusCode::OK)
            .header("content-type", "application/octet-stream")
            .body(Body::from(data))
            .unwrap()
    }
}

pub async fn blob_exists(
    Path((_name, digest)): Path<(String, String)>,
    State(state): State<SharedState>,
) -> Response {
    let resp = Response::builder();
    let registry = &state.read().unwrap().registry;
    let blob = registry.blobs.iter().find(|x| x.digest == digest);
    if blob.is_none() {
        resp.status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap()
    } else {
        resp.status(StatusCode::OK).body(Body::empty()).unwrap()
    }
}

pub async fn post_upload_blob(
    Path(name): Path<String>,
    headers: HeaderMap,
    Query(digest_payload): Query<DigestPayload>,
    State(state): State<SharedState>,
    body: Bytes,
) -> Response {
    if headers.contains_key("content-length") && headers.get("content-length").unwrap().eq("0") {
        // When requesting this function with a content-length == 0, generate a reference for chunck upload
        let registry = &mut state.write().unwrap();
        let start = SystemTime::now();
        let since_the_epoch = start
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        let request_chunck_upload = RequestChunkUpload {
            session_id: Uuid::new_v4(),
            close_session_id: Uuid::new_v4(),
            date: since_the_epoch,
            data: vec![],
        };
        registry
            .registry
            .request_chunk_uploads
            .push(request_chunck_upload.clone());
        return Response::builder()
            .header("range", "0-0")
            .header(
                "Location",
                format!(
                    "{}/v2/{}/blobs/uploads/{}",
                    ip, name, request_chunck_upload.session_id
                ),
            )
            .status(StatusCode::ACCEPTED)
            .body(Body::empty())
            .unwrap();
    }
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::from(""))
        .unwrap()
}

pub async fn put_upload_blob(
    Path((name, reference)): Path<(String, String)>,
    headers: HeaderMap,
    Query(digest_payload): Query<DigestPayload>,
    State(state): State<SharedState>,
    body: Bytes,
) -> Response {
    let registry = &mut state.write().unwrap().registry;
    let chunk = &mut registry
        .request_chunk_uploads
        .iter_mut()
        .find(|chunck_upload| {
            chunck_upload.close_session_id.to_string().eq(&reference)
                || chunck_upload.session_id.to_string().eq(&reference)
        });
    if chunk.is_none() {
        return Response::builder()
            .header("content-type", "application/json")
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .unwrap();
    }
    if !headers.get("content-length").is_none() {
        let content_length = headers
            .get("content-length")
            .unwrap()
            .to_str()
            .unwrap()
            .parse::<i32>()
            .unwrap();
        if content_length > 0 {
            chunk.as_mut().unwrap().data.extend(body.to_vec());
        }
        println!("Digest {:?}", digest_payload.digest);
        if digest_payload.digest.is_some() {
            registry.blobs.push(Blobs {
                digest: digest_payload.digest.clone().unwrap(),
                data: chunk.as_ref().unwrap().data.clone(),
            })
        }
        return Response::builder()
            .header(
                "location",
                format!(
                    "{}/v2/{}/blobs/{}",
                    ip,
                    name,
                    digest_payload.digest.unwrap()
                ),
            )
            .status(StatusCode::CREATED)
            .body(Body::empty())
            .unwrap();
    }

    Response::builder()
        .header("content-type", "application/json")
        .status(StatusCode::BAD_REQUEST)
        .body(Body::empty())
        .unwrap()
}

pub async fn patch_upload_blob(
    Path((name, reference)): Path<(String, String)>,
    headers: HeaderMap,
    State(state): State<SharedState>,
    body: Bytes,
) -> Response {
    let registry = &mut state.write().unwrap().registry;
    let chunck = &mut registry
        .request_chunk_uploads
        .iter_mut()
        .find(|chunck_upload| chunck_upload.session_id.to_string().eq(&reference));
    if chunck.is_none() {
        let request_error = RequestErrors {
            errors: vec![RequestError {
                code: "BLOB_UPLOAD_INVALID".to_string(),
                message: "blob upload invalid".to_string(),
                detail: "reference not found for blob upload".to_string(),
            }],
        };
        let json_payload = serde_json::to_string(&request_error).unwrap();
        return Response::builder()
            .header("content-type", "application/json")
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from(json_payload))
            .unwrap();
    }
    if !headers.get("content-length").is_none() {
        println!("*********************************");
        let content_length = headers
            .get("content-length")
            .unwrap()
            .to_str()
            .unwrap()
            .parse::<i32>()
            .unwrap();
        if content_length > 0 {
            chunck.as_mut().unwrap().data.extend(body.to_vec());
        }
        return Response::builder()
            .header("range", format!("0-{}", body.to_vec().len()))
            .header(
                "location",
                format!(
                    "{}/v2/{}/blobs/uploads/{}",
                    ip,
                    name,
                    chunck.as_ref().unwrap().close_session_id
                ),
            )
            .status(StatusCode::ACCEPTED)
            .body(Body::empty())
            .unwrap();
    }
    Response::builder()
        .header("content-type", "application/json")
        .status(StatusCode::BAD_REQUEST)
        .body(Body::empty())
        .unwrap()
}
