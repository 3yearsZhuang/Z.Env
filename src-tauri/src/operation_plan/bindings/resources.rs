//! 比较原状态后修改；补偿只撤回本次仍未被外部替换的资源。
use super::*;
use std::{fs::OpenOptions, io::Read};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum State {
    Missing,
    File { text: String },
    Link { target: PathBuf },
}
impl State {
    pub fn file(text: String) -> Self {
        Self::File { text }
    }
    pub(super) fn text(&self) -> Option<&str> {
        if let Self::File { text } = self {
            Some(text)
        } else {
            None
        }
    }
}
pub(super) fn capture(path: &Path) -> Result<State, PlanError> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(State::Missing),
        Err(_) => return Err(PlanError::EvidenceUnavailable),
    };
    if meta.file_type().is_symlink() {
        return Ok(State::Link {
            target: std::fs::read_link(path).map_err(|_| PlanError::EvidenceUnavailable)?,
        });
    }
    if !meta.is_file() {
        return Err(PlanError::BindingConflict);
    }
    let mut open = OpenOptions::new();
    open.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = open
        .open(path)
        .map_err(|_| PlanError::EvidenceUnavailable)?;
    if !file
        .metadata()
        .map_err(|_| PlanError::EvidenceUnavailable)?
        .is_file()
    {
        return Err(PlanError::EvidenceUnavailable);
    }
    let mut bytes = vec![];
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PlanError::EvidenceUnavailable)?;
    if bytes.len() > 1024 * 1024 {
        return Err(PlanError::EvidenceUnavailable);
    }
    Ok(State::file(
        String::from_utf8(bytes).map_err(|_| PlanError::EvidenceUnavailable)?,
    ))
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Resource {
    pub path: PathBuf,
    pub destination: PathBuf,
    #[serde(default)]
    pub follow_configuration: bool,
    pub before: State,
    pub after: State,
}
impl Resource {
    pub fn new(path: PathBuf, after: State) -> Result<Self, PlanError> {
        let destination = guards::destination(&path)?;
        let before = capture(&destination)?;
        Ok(Self {
            path,
            destination,
            follow_configuration: false,
            before,
            after,
        })
    }
    pub fn configuration(path: PathBuf, after: State) -> Result<Self, PlanError> {
        let destination = PathBuf::from(
            guards::configuration(&path)?
                .0
                .canonical_path
                .ok_or(PlanError::EvidenceUnavailable)?,
        );
        let before = capture(&destination)?;
        if matches!(before, State::Link { .. }) {
            return Err(PlanError::BindingConflict);
        }
        Ok(Self {
            path,
            destination,
            follow_configuration: true,
            before,
            after,
        })
    }
    pub fn check(&self, expected: &State) -> Result<(), PlanError> {
        let destination = if self.follow_configuration {
            PathBuf::from(
                guards::configuration(&self.path)?
                    .0
                    .canonical_path
                    .ok_or(PlanError::EvidenceUnavailable)?,
            )
        } else {
            guards::destination(&self.path)?
        };
        if destination != self.destination || capture(&self.destination)? != *expected {
            return Err(PlanError::BindingConflict);
        }
        Ok(())
    }
    pub fn replace(&self, expected: &State, next: &State) -> Result<(), PlanError> {
        self.check(expected)?;
        if expected == next {
            return Ok(());
        }
        if matches!(expected, State::File { .. }) || matches!(next, State::File { .. }) {
            if matches!(expected, State::Link { .. }) || matches!(next, State::Link { .. }) {
                return Err(PlanError::BindingConflict);
            }
            crate::config::replace_checked(&self.destination, expected.text(), next.text())
                .map_err(|_| PlanError::BindingConflict)?;
        } else {
            if matches!(expected, State::Link { .. }) {
                std::fs::remove_file(&self.destination).map_err(|_| PlanError::BindingConflict)?;
            }
            if let State::Link { target } = next {
                std::fs::create_dir_all(
                    self.destination
                        .parent()
                        .ok_or(PlanError::EvidenceUnavailable)?,
                )
                .map_err(|_| PlanError::BindingConflict)?;
                // 创建前再次检查父目录，绝不删除或替换普通目录。
                if guards::destination(&self.path)? != self.destination {
                    return Err(PlanError::BindingConflict);
                }
                crate::managed::create_symlink(target, &self.destination)
                    .map_err(|_| PlanError::BindingConflict)?;
            }
        }
        self.check(next)
    }
}
#[derive(Default)]
pub(in crate::operation_plan) struct Transaction {
    applied: Vec<Resource>,
    pub recovery_path: Option<PathBuf>,
}
impl Transaction {
    pub(super) fn apply(&mut self, resource: &Resource) -> Result<(), PlanError> {
        resource.check(&resource.before)?;
        // IO 失败也可能发生在原子替换之后，因此先登记待补偿资源。
        self.applied.push(resource.clone());
        resource.replace(&resource.before, &resource.after)
    }
    pub fn rollback(&mut self) -> Result<(), PlanError> {
        let mut conflict = false;
        for resource in self.applied.iter().rev() {
            if conflict && self.recovery_path.as_ref() == Some(&resource.path) {
                continue;
            }
            if resource.check(&resource.before).is_ok() {
                continue;
            }
            if resource.replace(&resource.after, &resource.before).is_err() {
                conflict = true;
            }
        }
        if conflict {
            Err(PlanError::BindingConflict)
        } else {
            self.applied.clear();
            Ok(())
        }
    }
}
