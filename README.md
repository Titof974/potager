# Example
```RUST
pub async fn with_status() -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let datetime = Local::now();


    let json_response = serde_json::json!({
        "status": "success".to_string()
    });

    Ok((StatusCode::NOT_FOUND, Json(json_response)))
}
````

# TODO
[] Change tag by name/tag hashmap<str,vec<str>>
[] Handle error correctly