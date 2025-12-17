use git2::Repository;
use std::path::Path;

pub fn is_git_repo(path: &Path) -> bool {
    Repository::discover(path).is_ok()
}

pub fn init_repo(path: &Path) -> anyhow::Result<Repository> {
    let repo = Repository::init(path)?;
    Ok(repo)
}
