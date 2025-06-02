use crate::{DgmResult, Deserialize, Serialize};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Load JSON file and deserialize to type T
pub fn load_json_file<T, P>(path: P) -> DgmResult<T>
where
    T: for<'de> Deserialize<'de>,
    P: AsRef<Path>,
{
    let content = fs::read_to_string(path.as_ref())?;
    let data: T = serde_json::from_str(&content)?;
    Ok(data)
}

/// Save data as JSON to file
pub fn save_json_file<T, P>(data: &T, path: P) -> DgmResult<()>
where
    T: Serialize,
    P: AsRef<Path>,
{
    let content = serde_json::to_string_pretty(data)?;
    fs::write(path.as_ref(), content)?;
    Ok(())
}

/// Generate a unique run ID based on current timestamp
pub fn generate_run_id() -> String {
    let now: DateTime<Utc> = Utc::now();
    now.format("%Y%m%d%H%M%S_%f").to_string()
}

/// Extract JSON content between markers in text
pub fn extract_json_between_markers(text: &str) -> Option<Value> {
    // Look for JSON between ```json and ``` markers
    if let Some(start) = text.find("```json") {
        if let Some(end) = text[start + 7..].find("```") {
            let json_str = &text[start + 7..start + 7 + end].trim();
            return serde_json::from_str(json_str).ok();
        }
    }

    // Look for JSON between { and } markers
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            if end > start {
                let json_str = &text[start..=end];
                return serde_json::from_str(json_str).ok();
            }
        }
    }

    None
}

/// Create directory if it doesn't exist
pub fn ensure_dir_exists<P: AsRef<Path>>(path: P) -> DgmResult<()> {
    fs::create_dir_all(path.as_ref())?;
    Ok(())
}

/// Check if file exists and is not empty
pub fn file_exists_and_not_empty<P: AsRef<Path>>(path: P) -> bool {
    if let Ok(metadata) = fs::metadata(path.as_ref()) {
        metadata.is_file() && metadata.len() > 0
    } else {
        false
    }
}

/// Copy file from source to destination
pub fn copy_file<P: AsRef<Path>>(src: P, dst: P) -> DgmResult<()> {
    fs::copy(src.as_ref(), dst.as_ref())?;
    Ok(())
}

/// Read file content as string
pub fn read_file_to_string<P: AsRef<Path>>(path: P) -> DgmResult<String> {
    let content = fs::read_to_string(path.as_ref())?;
    Ok(content)
}

/// Write string content to file
pub fn write_string_to_file<P: AsRef<Path>>(content: &str, path: P) -> DgmResult<()> {
    fs::write(path.as_ref(), content)?;
    Ok(())
}
