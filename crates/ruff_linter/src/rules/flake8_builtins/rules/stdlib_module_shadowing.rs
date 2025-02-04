use std::path::{Component, Path, PathBuf};

use itertools::Itertools;
use ruff_diagnostics::{Diagnostic, Violation};
use ruff_macros::{derive_message_formats, ViolationMetadata};
use ruff_python_ast::PySourceType;
use ruff_python_stdlib::path::is_module_file;
use ruff_python_stdlib::sys::is_known_standard_library;
use ruff_text_size::TextRange;

use crate::rules::flake8_builtins;
use crate::settings::types::PythonVersion;

/// ## What it does
/// Checks for modules that use the same names as Python standard-library
/// modules.
///
/// ## Why is this bad?
/// Reusing a standard-library module name for the name of a module increases
/// the difficulty of reading and maintaining the code, and can cause
/// non-obvious errors. Readers may mistake the first-party module for the
/// standard-library module and vice versa.
///
/// Standard-library modules can be marked as exceptions to this rule via the
/// [`lint.flake8-builtins.builtins-allowed-modules`] configuration option.
///
/// This rule is not applied to stub files, as the name of a stub module is out
/// of the control of the author of the stub file. Instead, a stub should aim to
/// faithfully emulate the runtime module it is stubbing.
///
/// As of Python 3.13, errors from modules that use the same name as
/// standard-library modules now display a custom message.
///
/// ## Example
///
/// ```console
/// $ touch random.py
/// $ python3 -c 'from random import choice'
/// Traceback (most recent call last):
///   File "<string>", line 1, in <module>
///     from random import choice
/// ImportError: cannot import name 'choice' from 'random' (consider renaming '/random.py' since it has the same name as the standard library module named 'random' and prevents importing that standard library module)
/// ```
///
/// ## Options
/// - `lint.flake8-builtins.builtins-allowed-modules`
#[derive(ViolationMetadata)]
pub(crate) struct StdlibModuleShadowing {
    name: String,
}

impl Violation for StdlibModuleShadowing {
    #[derive_message_formats]
    fn message(&self) -> String {
        let StdlibModuleShadowing { name } = self;
        format!("Module `{name}` shadows a Python standard-library module")
    }
}

/// A005
pub(crate) fn stdlib_module_shadowing(
    path: &Path,
    settings: &flake8_builtins::settings::Settings,
    target_version: PythonVersion,
    project_root: &Path,
    src: &[PathBuf],
) -> Option<Diagnostic> {
    if !PySourceType::try_from_path(path).is_some_and(PySourceType::is_py_file) {
        return None;
    }

    let mut path = PathBuf::from(path);

    // strip src directories from the path. sort by descending length to strip the longest prefix
    // available
    for s in src.iter().sorted_by_key(|p| p.as_os_str().len()).rev() {
        if let Ok(rest) = path.strip_prefix(s) {
            path = rest.into();
            break;
        }
    }

    // strip the project root from the path
    if let Ok(rest) = path.strip_prefix(project_root) {
        path = rest.into();
    }

    // if `path` is a file like `__init__.py` use its parent directory as the module name, otherwise
    // strip the `.py` extension
    if is_module_file(&path) {
        path = path.parent()?.into();
    } else {
        path.set_extension("");
    };

    // in strict mode, reject based on the final component only. for example, a module named
    // `utils.logging` is rejected in strict mode but allowed in non-strict mode
    let module_name = if settings.builtins_strict_checking {
        path.file_name()?.to_string_lossy().to_string()
    } else {
        path.components()
            .filter_map(|c| match c {
                Component::Normal(part) => Some(part.to_string_lossy()),
                _ => None,
            })
            .join(".")
    };

    if !is_known_standard_library(target_version.minor(), &module_name) {
        return None;
    }

    // Shadowing private stdlib modules is okay.
    // https://github.com/astral-sh/ruff/issues/12949
    if module_name.starts_with('_') && !module_name.starts_with("__") {
        return None;
    }

    if settings
        .builtins_allowed_modules
        .iter()
        .any(|allowed_module| allowed_module == &module_name)
    {
        return None;
    }

    Some(Diagnostic::new(
        StdlibModuleShadowing { name: module_name },
        TextRange::default(),
    ))
}
