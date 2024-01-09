use axum::{
    body::{self, Body, Bytes},
    extract::{DefaultBodyLimit, FromRef, MatchedPath, Multipart, Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{delete, get, head, patch, post, put},
    Error, Json, Router,
};
use axum_extra::routing::RouterExt;
use chrono::Local;
use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use sha256::digest;
use std::{
    collections::HashMap,
    default, fmt,
    str::FromStr,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::ReadBuf;
use tower_http::{classify::ServerErrorsFailureClass, trace::TraceLayer};
use tracing::{debug, info, info_span, Span};
use uuid::Uuid;

type SharedState = Arc<RwLock<AppState>>;

#[derive(Default)]
struct AppState {
    db: HashMap<String, Bytes>,
    tickets: HashMap<String, Duration>,
    registry: Registry,
}

#[derive(Default)]
struct Registry {
    blobs: Vec<Blobs>,
    manifests: Vec<Manifest>,
    request_chunk_uploads: Vec<RequestChunkUpload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u8,
    media_type: String,
    #[serde(skip_serializing)] 
    digest: Option<String>,
    config: Layer,
    layers: Vec<Layer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subject: Option<Layer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    annotations: Option<HashMap<String, String>>,
    #[serde(skip_serializing)] 
    tags: Option<Vec<String>>,
    #[serde(skip_serializing, skip_deserializing)]
    data: Option<Vec<u8>>
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Layer {
    media_type: String,
    digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Blobs {
    digest: String,
    data: Vec<u8>,
}

#[derive(Debug, Clone)]
struct RequestChunkUpload {
    session_id: Uuid,
    close_session_id: Uuid,
    date: Duration,
    data: Vec<u8>,
}
//const ip: &str= "http://192.168.50.108:3000";
const ip: &str = "http://172.26.16.1:3000";

#[tokio::main]
async fn main() {
    let shared_state = SharedState::default();

    // initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();
    let v2 = Router::new()
        .route("/v2/:name/blobs/:digest", get(get_blob))
        .route("/v2/:name/blobs/:digest", head(blob_exists))
        .route("/v2/:name/blobs/:digest", delete(with_status))
        .route("/v2/:name/blobs/uploads/", post(upload_blob))
        .route("/v2/:name/blobs/uploads/:reference", put(put_upload_blob))
        .route(
            "/v2/:name/blobs/uploads/:reference",
            patch(patch_upload_blob),
        )
        .route("/v2/:name/manifests/:reference", get(get_manifest))
        .route("/v2/:name/manifests/:reference", head(manifest_exists))
        .route("/v2/:name/manifests/:reference", put(put_upload_manifest))
        .route("/v2/:name/manifests/:reference", delete(with_status))
        .route("/v2/:name/tags/list", get(with_status))
        // end-1 S: 200, F: 404/401
        .route_with_tsr("/v2/", get(with_status_ok))
        .layer(DefaultBodyLimit::max(999999910240000))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<_>| {
                    // Log the matched route's path (with placeholders not filled in).
                    // Use request.uri() or OriginalUri if you want the real path.
                    let matched_path = request
                        .extensions()
                        .get::<MatchedPath>()
                        .map(MatchedPath::as_str);

                    info_span!(
                        "http_request",
                        method = ?request.method(),
                        matched_path,
                        some_other_field = tracing::field::Empty,
                    )
                })
                .on_request(|_request: &Request<_>, _span: &Span| {
                    // You can use `_span.record("some_other_field", value)` in one of these
                    // closures to attach a value to the initially empty field in the info_span
                    // created above.
                    info!("{} {:?}", _request.uri(), _request.headers());
                })
                .on_response(|_response: &Response, _latency: Duration, _span: &Span| {
                    info!(
                        "Response status {} {:?}",
                        _response.status(),
                        _response.headers()
                    );
                    // ...
                })
                .on_body_chunk(|_chunk: &Bytes, _latency: Duration, _span: &Span| {
                    // ...
                })
                .on_eos(
                    |_trailers: Option<&HeaderMap>, _stream_duration: Duration, _span: &Span| {
                        // ...
                    },
                )
                .on_failure(
                    |_error: ServerErrorsFailureClass, _latency: Duration, _span: &Span| {
                        info!("{}", _error);
                    },
                ),
        );

    // build our application with a route
    let app = Router::new().nest("/", v2).with_state(shared_state);

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn blob_exists(
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
async fn get_blob(
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

fn generate_json_error(code: String, message: String, detail: String) -> String {
    let errors = RequestErrors {
        errors: vec![RequestError {
            code,
            message,
            detail,
        }],
    };
    serde_json::to_string(&errors).expect("Can't serialize error")
}

async fn get_manifest(
    Path((_name, digest)): Path<(String, String)>,
    State(state): State<SharedState>,
) -> Response {
    let full_name = format!("{}:{}", _name, digest);
    let resp = Response::builder();
    let registry = &state.read().unwrap().registry;
    let manifest = registry.manifests.iter().find(|x| {
        x.digest.clone().unwrap_or_default() == digest
            || x.tags
                .clone()
                .unwrap_or_default()
                .iter()
                .any(|tag| tag.eq(full_name.as_str()))
    });

    if manifest.is_none() {
        resp.header("content-type", "application/json")
            .status(StatusCode::NOT_FOUND)
            .body(Body::from(generate_json_error(
                "MANIFEST_UNKNOWN".to_string(),
                format!("Manifest {} unknown", full_name),
                "".to_string(),
            )))
            .unwrap()
    } else {
        // let data = blob.unwrap().data.to_vec();
        let data = serde_json::to_string(manifest.unwrap()).expect("Can't serialize manifest");
        resp.status(StatusCode::OK)
            .header(
                "content-type",
                "application/vnd.docker.distribution.manifest.v2+json",
            )
            .body(Body::from(data))
            .unwrap()
    }
}

async fn put_upload_manifest(
    Path((name, reference)): Path<(String, String)>,
    headers: HeaderMap,
    State(state): State<SharedState>,
    body: Bytes,
) -> Response {
    let registry = &mut state.write().unwrap().registry;
    let sha = digest(body.clone().to_vec());
    let digest = format!("sha256:{}", sha);
    debug!("Digest: {:?}", digest);
    debug!("BODY: {:?}", String::from_utf8_lossy(body.to_vec().as_slice()));
    let manifest = registry
        .manifests
        .iter()
        .find(|x| x.digest.clone().unwrap_or_default() == digest);
    if manifest.is_none() {
        let uploaded_manifest = &mut serde_json::from_slice::<Manifest>(body.to_vec().as_slice())
            .expect("Can't deserialize manifest");
        uploaded_manifest.digest = Some(digest.clone());
        uploaded_manifest.tags = Some(vec![format!("{}:{}", name, reference)]);
        uploaded_manifest.data = Some(body.to_vec());
        let _ = &mut registry.manifests.push(uploaded_manifest.clone());
        debug!("{:?}", uploaded_manifest);
    }

    Response::builder()
        .header("content-type", "application/json")
        .header("Docker-Content-Digest", digest.clone())
        .header(
            "Location",
            format!("{}/v2/{}/manifest/{}", ip, name, digest),
        )
        .status(StatusCode::CREATED)
        .body(Body::empty())
        .unwrap()
}

pub async fn upload_blob_with_reference(
    Path((name, reference)): Path<(String, String)>,
    headers: HeaderMap,
    Query(digest_payload): Query<DigestPayload>,
    State(state): State<SharedState>,
    body: Bytes,
) -> Response {
    if !state
        .read()
        .unwrap()
        .tickets
        .contains_key(reference.as_str())
    {
        let data = RequestErrors {
            errors: vec![RequestError {
                code: "BLOB_UPLOAD_INVALID".to_string(),
                message: "No reference found for blob".to_string(),
                detail: "".to_string(),
            }],
        };
        let j = serde_json::to_string(&data).unwrap();
        return Response::builder()
            .header("content-type", "application/json")
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from(j))
            .unwrap();
    }
    let sha = digest(body.to_vec());
    let data = RequestErrors {
        errors: vec![RequestError {
            code: "DIGEST_INVALID".to_string(),
            message: "provided digest did not match uploaded content".to_string(),
            detail: "".to_string(),
        }],
    };
    let j = serde_json::to_string(&data).unwrap();
    let key = format!("sha256:{}", sha);

    // Compare payload digest with query digest
    if !key.eq(&digest_payload.digest.unwrap()) {
        return Response::builder()
            .header("content-type", "application/json")
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from(j))
            .unwrap();
    }
    let app = &mut state.write().unwrap();
    if !app.db.contains_key(key.as_str()) {
        println!("Inserting data in db");
        app.db.insert(key, body);
        println!("Deleting reference");
        app.tickets
            .remove(reference.as_str())
            .expect("wtf is going on");
        println!("Reference deleted");
    }
    Response::builder()
        .status(StatusCode::ACCEPTED)
        .body(Body::from(""))
        .unwrap()
}

pub async fn patch_upload_blob_with_reference(
    Path((name, reference)): Path<(String, String)>,
    headers: HeaderMap,
    State(state): State<SharedState>,
    body: Bytes,
) -> Response {
    println!("{:?}", headers);
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    let my_uuid = Uuid::new_v4();
    let sha = digest(body.to_vec());
    &mut state
        .write()
        .unwrap()
        .tickets
        .insert(format!("{}", my_uuid), since_the_epoch);
    Response::builder()
        .header(
            "Location",
            format!("{}/v2/{}/blobs/uploads/{}", ip, name, my_uuid),
        )
        .status(StatusCode::ACCEPTED)
        .body(Body::from(""))
        .unwrap()
}

async fn put_upload_blob(
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

async fn patch_upload_blob(
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

async fn upload_blob(
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
            .header(
                "Location",
                format!(
                    "{}/v2/{}/blobs/uploads/{}",
                    ip, name, request_chunck_upload.session_id
                ),
            )
            .status(StatusCode::ACCEPTED)
            .body(Body::from(""))
            .unwrap();
    }
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::from(""))
        .unwrap()
}

pub async fn manifest_exists(
    Path((name, digest)): Path<(String, String)>,
    State(state): State<SharedState>,
) -> Result<impl IntoResponse, StatusCode> {
    let full_name = format!("{}:{}", name, digest);
    debug!("Searching for manifest {}", full_name);
    let resp = Response::builder();
    let registry = &state.read().unwrap().registry;
    debug!("MANIFESTS: {:?}", registry.manifests);
    let manifest = registry.manifests.iter().find(|x| {
        x.digest.clone().unwrap_or_default().eq(&digest)
            || x.tags
                .clone()
                .unwrap_or_default()
                .iter()
                .any(|tag| tag.eq(full_name.as_str()))
    });

    if manifest.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(StatusCode::OK)
}

pub async fn with_status_ok() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(
            "Docker-Distribution-API-Version",
            "application/json; charset=utf-8",
        )
        .header("Content-Type", "registry/2.0")
        .body(Body::from("{}"))
        .unwrap()
}

pub async fn with_status() -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let datetime = Local::now();

    let json_response = serde_json::json!({
        "status": "success".to_string()
    });

    Ok((StatusCode::NOT_FOUND, Json(json_response)))
}

#[derive(Serialize, Deserialize, Debug)]
struct RequestErrors {
    errors: Vec<RequestError>,
}

#[derive(Serialize, Deserialize, Debug)]
struct RequestError {
    code: String,
    message: String,
    detail: String,
}

#[derive(Deserialize)]
struct DigestPayload {
    #[serde(default, deserialize_with = "empty_string_as_none")]
    digest: Option<String>,
}

fn empty_string_as_none<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: fmt::Display,
{
    let opt = Option::<String>::deserialize(de)?;
    match opt.as_deref() {
        None | Some("") => Ok(None),
        Some(s) => FromStr::from_str(s).map_err(de::Error::custom).map(Some),
    }
}

fn nullable_layer<'de, D>(de: D) -> Result<Layer, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<Layer> = Option::deserialize(de)?;
    Ok(opt.unwrap_or_default())
}
