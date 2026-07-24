use serde_json::{Value, json};

pub async fn fetch_ad_list() -> anyhow::Result<Value> {
    Ok(empty_ad_payload())
}

pub async fn fetch_ad_list_from_urls<S>(urls: &[S]) -> anyhow::Result<Value>
where
    S: AsRef<str>,
{
    let _ = urls;
    Ok(empty_ad_payload())
}

fn empty_ad_payload() -> Value {
    json!({ "version": 1, "ads": [] })
}
