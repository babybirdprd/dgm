use dgm::{
    dgm::{Archive, DgmConfig, EvolutionStrategy},
    utils::{generate_run_id, load_json_file},
};
use tempfile::tempdir;

#[test]
fn test_dgm_config_validation() {
    let config = DgmConfig::new(
        10,
        2,
        2,
        "random".to_string(),
        None,
        "keep_all".to_string(),
        1,
        false,
        false,
        false,
        0.1,
        false,
        None,
    );

    assert!(config.validate().is_ok());

    // Test invalid config
    let invalid_config = DgmConfig::new(
        0, // Invalid: must be > 0
        2,
        2,
        "random".to_string(),
        None,
        "keep_all".to_string(),
        1,
        false,
        false,
        false,
        0.1,
        false,
        None,
    );

    assert!(invalid_config.validate().is_err());
}

#[test]
fn test_archive_operations() {
    let mut archive = Archive::new();
    
    assert_eq!(archive.len(), 1);
    assert!(archive.contains("initial"));
    
    archive.add_commit("commit1".to_string());
    assert_eq!(archive.len(), 2);
    assert!(archive.contains("commit1"));
    
    // Adding the same commit again should not increase length
    archive.add_commit("commit1".to_string());
    assert_eq!(archive.len(), 2);
}

#[test]
fn test_evolution_strategy() {
    let strategy = EvolutionStrategy::new("random".to_string());
    let archive = Archive::new();
    let temp_dir = tempdir().unwrap();
    
    // This should not panic even with empty archive
    let result = strategy.choose_selfimproves(
        &archive,
        1,
        temp_dir.path(),
        None,
        false,
    );
    
    // Should return empty list since no valid candidates
    assert!(result.is_ok());
    let entries = result.unwrap();
    assert!(entries.is_empty());
}

#[test]
fn test_generate_run_id() {
    let id1 = generate_run_id();
    let id2 = generate_run_id();
    
    assert_ne!(id1, id2);
    assert!(id1.len() > 10);
    assert!(id2.len() > 10);
}

#[test]
fn test_load_json_file() {
    let temp_dir = tempdir().unwrap();
    let test_file = temp_dir.path().join("test.json");
    
    // Create a test JSON file
    let test_data = r#"{"test": "value", "number": 42}"#;
    std::fs::write(&test_file, test_data).unwrap();
    
    // Test loading
    let result: serde_json::Value = load_json_file(&test_file).unwrap();
    assert_eq!(result["test"], "value");
    assert_eq!(result["number"], 42);
}

#[tokio::test]
async fn test_dgm_runner_creation() {
    // This test requires API keys to be set, so we'll test the error case
    let result = dgm::dgm::DgmRunner::new(
        1,
        1,
        1,
        "random".to_string(),
        None,
        "keep_all".to_string(),
        1,
        false,
        false,
        false,
        0.1,
        false,
        None,
    );
    
    // Should fail due to missing API keys
    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_msg = error.to_string();
    assert!(error_msg.contains("API key"));
}
