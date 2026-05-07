use nu_engine::command_prelude::*;
use nu_protocol::{NuGlob, report_shell_error, shell_error::generic::GenericError};
use uu_mkdir::mkdir;
use uucore::{localized_help_template, translate};

#[derive(Clone)]
pub struct UMkdir;

const IS_RECURSIVE: bool = true;
const DEFAULT_MODE: u32 = 0o777;

#[cfg(target_family = "unix")]
fn get_mode() -> u32 {
    !nu_system::get_umask() & DEFAULT_MODE
}

#[cfg(not(target_family = "unix"))]
fn get_mode() -> u32 {
    DEFAULT_MODE
}

impl Command for UMkdir {
    fn name(&self) -> &str {
        "mkdir"
    }

    fn description(&self) -> &str {
        "Create directories, with intermediary directories if required using uutils/coreutils mkdir."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "directory",
            "folder",
            "create",
            "make_dirs",
            "coreutils",
            "md",
        ]
    }

    fn signature(&self) -> Signature {
        Signature::build("mkdir")
            .input_output_types(vec![(Type::Nothing, Type::Nothing)])
            .rest(
                "rest",
                SyntaxShape::OneOf(vec![SyntaxShape::GlobPattern, SyntaxShape::Directory]),
                "The name(s) of the path(s) to create.",
            )
            .switch(
                "verbose",
                "Print a message for each created directory.",
                Some('v'),
            )
            .category(Category::FileSystem)
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        // setup the uutils error translation
        let _ = localized_help_template("mkdir");

        let cwd = engine_state.cwd(Some(stack))?.into_std_path_buf();
        let mut directories = call
            .rest::<Spanned<NuGlob>>(engine_state, stack, 0)?
            .into_iter()
            .map(|dir| nu_path::expand_path_with(dir.item.as_ref(), &cwd, dir.item.is_expand()))
            .peekable();

        let is_verbose = call.has_flag(engine_state, stack, "verbose")?;

        if directories.peek().is_none() {
            return Err(ShellError::MissingParameter {
                param_name: "requires directory paths".to_string(),
                span: call.head,
            });
        }

        let config = uu_mkdir::Config {
            recursive: IS_RECURSIVE,
            mode: get_mode(),
            verbose: is_verbose,
            set_security_context: false,
            context: None,
        };

        let mut verbose_out = String::new();
        let mut cmd_result = Ok(());
        for dir in directories {
            engine_state.signals().check(&call.head)?;

            if let Err(error) = mkdir(&dir, &config) {
                let err = ShellError::Generic(GenericError::new_internal(
                    format!("{error}"),
                    translate!(&error.to_string()),
                ));

                if cmd_result.is_ok() {
                    cmd_result = Err(err);
                } else {
                    report_shell_error(Some(stack), engine_state, &err);
                }

                continue;
            }

            if is_verbose {
                verbose_out.push_str(
                    format!(
                        "{} ",
                        &dir.as_path()
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                    )
                    .as_str(),
                );
            }
        }

        cmd_result?;

        if is_verbose {
            Ok(PipelineData::value(
                Value::string(verbose_out.trim(), call.head),
                None,
            ))
        } else {
            Ok(PipelineData::empty())
        }
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Make a directory named foo.",
                example: "mkdir foo",
                result: None,
            },
            Example {
                description: "Make multiple directories and show the paths created.",
                example: "mkdir -v foo/bar foo2",
                result: None,
            },
        ]
    }
}
