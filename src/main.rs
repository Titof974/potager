use std::{time::Duration, collections::HashMap, str::FromStr, fmt, sync::{Arc, RwLock}};
use sha256::digest;
use axum::{
    routing::{get, post, head, put, delete},
    http::{StatusCode, Uri, header, Request, HeaderMap},
    Json, Router,
    response::{IntoResponse, Response}, body::{Body, self, Bytes}, Error, extract::{Path, MatchedPath, State, Query, Multipart, DefaultBodyLimit, FromRef}
};
use chrono::Local;
use serde::{Deserialize, Serialize, Deserializer, de};
use tokio::io::ReadBuf;
use tracing::{info, info_span, Span};
use tower_http::{classify::ServerErrorsFailureClass, trace::TraceLayer};
use uuid::Uuid;

type SharedState = Arc<RwLock<AppState>>;

#[derive(Default)]
struct AppState {
    db: HashMap<String, Bytes>,
}


#[tokio::main]
async fn main() {
    let shared_state = SharedState::default();

    // initialize tracing
    tracing_subscriber::fmt()
    .with_max_level(tracing::Level::DEBUG)
    .init();
    let v2 = Router::new()
        .route("/:name/blobs/:digest", get(blob_exists))
        .route("/:name/blobs/:digest", head(blob_exists))
        .route("/:name/blobs/:digest", delete(with_status))
        .route("/:name/blobs/uploads/", post(upload_blob))
        .route("/:name/blobs/uploads/", put(with_status))
        .route("/:name/manifests/:reference", get(with_status))
        .route("/:name/manifests/:reference", head(manifest_exists))
        .route("/:name/manifests/:reference", put(with_status))
        .route("/:name/manifests/:reference", delete(with_status))
        .route("/:name/tags/list", get(with_status))
        // end-1 S: 200, F: 404/401
        .route("/", get(with_status))
        .layer(DefaultBodyLimit::max(10240000));
        // .layer(
        //     TraceLayer::new_for_http()
        //         .make_span_with(|request: &Request<_>| {
        //             // Log the matched route's path (with placeholders not filled in).
        //             // Use request.uri() or OriginalUri if you want the real path.
        //             let matched_path = request
        //                 .extensions()
        //                 .get::<MatchedPath>()
        //                 .map(MatchedPath::as_str);

        //             info_span!(
        //                 "http_request",
        //                 method = ?request.method(),
        //                 matched_path,
        //                 some_other_field = tracing::field::Empty,
        //             )
        //         })
        //         .on_request(|_request: &Request<_>, _span: &Span| {
        //             // You can use `_span.record("some_other_field", value)` in one of these
        //             // closures to attach a value to the initially empty field in the info_span
        //             // created above.
        //         })
        //         .on_response(|_response: &Response, _latency: Duration, _span: &Span| {
        //             // ...
        //         })
        //         .on_body_chunk(|_chunk: &Bytes, _latency: Duration, _span: &Span| {
        //             // ...
        //         })
        //         .on_eos(
        //             |_trailers: Option<&HeaderMap>, _stream_duration: Duration, _span: &Span| {
        //                 // ...
        //             },
        //         )
        //         .on_failure(
        //             |_error: ServerErrorsFailureClass, _latency: Duration, _span: &Span| {
        //                 info!("{}", _error);
        //             },
        //         ),
        // );

    // build our application with a route
    let app = Router::new()
        .nest("/v2", v2)        .with_state(shared_state)        ;

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}


pub async fn upload_blob(Path(name): Path<String>, headers: HeaderMap, Query(digestPayload): Query<DigestPayload>, State(state): State<SharedState>, body: Bytes) -> Response {
    // println!("{:?}", headers);
    // println!("{:?}", digestPayload.digest);
    // println!("{:?}", body);
    let sha = digest(body.to_vec());
    // println!("{:?}", state.read().unwrap().db);

    let my_uuid = Uuid::new_v4();
    let key = format!("sha256:{}", sha);
    let app = &mut state.write().unwrap();
    if !app.db.contains_key(key.as_str()) {
        app.db.insert(key, body);
    }
    Response::builder()
    .status(StatusCode::ACCEPTED)
    .header("Location", format!("{}", my_uuid))
    .body(Body::from(""))
    .unwrap()
}

pub async fn blob_exists(Path((name, digest)): Path<(String, String)>, State(state): State<SharedState>) ->  Response {
    // info!("Searching for blob {}:{} {:?}", name, digest, state.read().unwrap().db);
    let mut resp =     Response::builder();
    if name.eq("registry") && state.read().unwrap().db.contains_key(digest.as_str()) {
        let data = state.read().unwrap().db.get(digest.as_str()).unwrap().to_vec();
        return resp
        .status(StatusCode::OK)
        .header("content-type", "application/octet-stream")
        .body(Body::from(data))
        .unwrap();
    }
    return resp.status(StatusCode::NOT_FOUND)
    .body(Body::empty())
    .unwrap();
}


pub async fn manifest_exists(Path((name, reference)): Path<(String, String)>) ->  Result<impl IntoResponse, StatusCode> {
    info!("Searching for manifest {}:{}", name, reference);
    if name.eq("registry") && reference.eq("latest") {
        return Ok(StatusCode::OK);
    }
    Err(StatusCode::NOT_FOUND)
}

pub async fn with_status() -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let datetime = Local::now();


    let json_response = serde_json::json!({
        "status": "success".to_string()
    });

    Ok((StatusCode::NOT_FOUND, Json(json_response)))
}
async fn create_user(
    // this argument tells axum to parse the request body
    // as JSON into a `CreateUser` type
    Json(payload): Json<CreateUser>,
) -> (StatusCode, Json<User>) {
    // insert your application logic here
    let user = User {
        id: 1337,
        username: payload.username,
    };

    // this will be converted into a JSON response
    // with a status code of `201 Created`
    (StatusCode::CREATED, Json(user))
}

// the input to our `create_user` handler
#[derive(Deserialize)]
struct CreateUser {
    username: String,
}

// the output to our `create_user` handler
#[derive(Serialize)]
struct User {
    id: u64,
    username: String,
}

#[derive(Deserialize)]
struct RequestError {
    code: String,
    message: String,
    detail: String
}

#[derive(Deserialize)]
struct DigestPayload {
    #[serde(default, deserialize_with = "empty_string_as_none")]
    digest: Option<String>
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