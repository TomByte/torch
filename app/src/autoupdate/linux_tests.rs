use super::*;

#[test]
fn test_package_and_repo_names() {
    assert_eq!(PackageManager::package_name(), "torch");
    assert_eq!(PackageManager::repo_name(), "torch");
}
