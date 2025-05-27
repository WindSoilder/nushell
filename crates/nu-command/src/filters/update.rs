use nu_engine::{ClosureEval, ClosureEvalOnce, command_prelude::*};
use nu_protocol::{ast::PathMember, engine::Closure};

#[derive(Clone)]
pub struct Update;

impl Command for Update {
    fn name(&self) -> &str {
        "update"
    }

    fn signature(&self) -> Signature {
        Signature::build("update")
            .input_output_types(vec![
                (Type::record(), Type::record()),
                (Type::table(), Type::table()),
                (
                    Type::List(Box::new(Type::Any)),
                    Type::List(Box::new(Type::Any)),
                ),
            ])
            .required(
                "field",
                SyntaxShape::CellPath,
                "The name of the column to update.",
            )
            .required(
                "replacement value",
                SyntaxShape::Any,
                "The new value to give the cell(s), or a closure to create the value.",
            )
            .allow_variants_without_examples(true)
            .category(Category::Filters)
    }

    fn description(&self) -> &str {
        "Update an existing column to have a new value."
    }

    fn extra_description(&self) -> &str {
        "When updating a column, the closure will be run for each row, and the current row will be passed as the first argument. \
Referencing `$in` inside the closure will provide the value at the column for the current row.

When updating a specific index, the closure will instead be run once. The first argument to the closure and the `$in` value will both be the current value at the index."
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        update(engine_state, stack, call, input)
    }

    fn examples(&self) -> Vec<Example> {
        vec![
            Example {
                description: "Update a column value",
                example: "{'name': 'nu', 'stars': 5} | update name 'Nushell'",
                result: Some(Value::test_record(record! {
                    "name" =>  Value::test_string("Nushell"),
                    "stars" => Value::test_int(5),
                })),
            },
            Example {
                description: "Use a closure to alter each value in the 'authors' column to a single string",
                example: "[[project, authors]; ['nu', ['Andrés', 'JT', 'Yehuda']]] | update authors {|row| $row.authors | str join ',' }",
                result: Some(Value::test_list(vec![Value::test_record(record! {
                    "project" => Value::test_string("nu"),
                    "authors" => Value::test_string("Andrés,JT,Yehuda"),
                })])),
            },
            Example {
                description: "Implicitly use the `$in` value in a closure to update 'authors'",
                example: "[[project, authors]; ['nu', ['Andrés', 'JT', 'Yehuda']]] | update authors { str join ',' }",
                result: Some(Value::test_list(vec![Value::test_record(record! {
                    "project" => Value::test_string("nu"),
                    "authors" => Value::test_string("Andrés,JT,Yehuda"),
                })])),
            },
            Example {
                description: "Update a value at an index in a list",
                example: "[1 2 3] | update 1 4",
                result: Some(Value::test_list(vec![
                    Value::test_int(1),
                    Value::test_int(4),
                    Value::test_int(3),
                ])),
            },
            Example {
                description: "Use a closure to compute a new value at an index",
                example: "[1 2 3] | update 1 {|i| $i + 2 }",
                result: Some(Value::test_list(vec![
                    Value::test_int(1),
                    Value::test_int(4),
                    Value::test_int(3),
                ])),
            },
        ]
    }
}

fn update_value_with_closure(
    engine_state: &EngineState,
    stack: &mut Stack,
    mut value: Value,
    closure: Closure,
    members: Vec<PathMember>,
    head: Span,
) -> Result<Value, ShellError> {
    // follow cell_path until last_cell(exclusive).
    let (mut last_value, last_member, prev_members) = match members.split_last() {
        None => {
            update_single_value_by_closure(
                &mut value,
                ClosureEvalOnce::new(engine_state, stack, closure),
                head,
                &members,
                false,
            )?;
            return Ok(value);
        }
        Some((last, members)) => {
            if members.is_empty() {
                (value.clone(), last, members)
            } else {
                (value.follow_cell_path(members)?.into_owned(), last, members)
            }
        }
    };

    // NOTE: it's really tricky..
    // If the input value is list, then `last_value` will always be a list, and
    // it doesn't refer to the real last value, here are the difference:
    // e.g:
    // {a: [{b: 1}, {b: 2}]} --> [{b: 1}, {b: 2}]
    // [{a: [{b: 1}, {b: 2}]}] --> [[{b: 1}, {b: 2}]]
    // for the later one, nushell needs to check nested elements.
    let is_value_list = matches!(value, Value::List { .. });
    let is_first_member_int = matches!(members[0], PathMember::Int { .. });
    // update `last_value` first, then set last value back to `value`.
    match (last_member, &mut last_value) {
        (PathMember::String { .. }, Value::List { vals, .. }) => {
            let mut closure = ClosureEval::new(engine_state, stack, closure);
            if is_value_list && !is_first_member_int {
                // `vals` is always a list, it's required to check elements
                // to get **real last value**.
                for row in vals {
                    match row {
                        Value::List { vals: v, .. } => {
                            for inner_row in v {
                                update_value_by_closure(
                                    inner_row,
                                    &mut closure,
                                    head,
                                    &[last_member.clone()],
                                    false,
                                )?
                            }
                        }
                        _ => update_value_by_closure(
                            row,
                            &mut closure,
                            head,
                            &[last_member.clone()],
                            false,
                        )?,
                    }
                }
            } else {
                for val in vals {
                    update_value_by_closure(
                        val,
                        &mut closure,
                        head,
                        &[last_member.clone()],
                        false,
                    )?;
                }
            }
            if !prev_members.is_empty() {
                if is_value_list && !is_first_member_int {
                    let rows = match value {
                        Value::List { ref mut vals, .. } => vals,
                        _ => panic!(
                            "Already checked the value is a list, please fire an issue if you get this message"
                        ),
                    };
                    let last_value_rows = match last_value {
                        Value::List { vals, .. } => vals,
                        _ => panic!(
                            "The input value is a list, so last_value must be a list, please fire an issue if you get this message"
                        ),
                    };
                    for (value_row, last_value_row) in rows.iter_mut().zip(last_value_rows) {
                        value_row.update_data_at_cell_path(prev_members, last_value_row)?;
                    }
                } else {
                    value.update_data_at_cell_path(prev_members, last_value)?;
                }
            } else {
                // no prev_members, so last_value is actually
                // the value itself.
                value = last_value;
            }
        }
        (first, _) => {
            update_single_value_by_closure(
                &mut value,
                ClosureEvalOnce::new(engine_state, stack, closure),
                head,
                &members,
                matches!(first, PathMember::Int { .. }),
            )?;
        }
    }
    Ok(value)
}

fn update(
    engine_state: &EngineState,
    stack: &mut Stack,
    call: &Call,
    input: PipelineData,
) -> Result<PipelineData, ShellError> {
    let head = call.head;
    let cell_path: CellPath = call.req(engine_state, stack, 0)?;
    let replacement: Value = call.req(engine_state, stack, 1)?;

    match input {
        PipelineData::Value(mut value, metadata) => {
            if let Value::Closure { val: closure, .. } = replacement {
                value = update_value_with_closure(
                    engine_state,
                    stack,
                    value,
                    *closure,
                    cell_path.members,
                    head,
                )?;
            } else {
                value.update_data_at_cell_path(&cell_path.members, replacement)?;
            }
            Ok(value.into_pipeline_data_with_metadata(metadata))
        }
        PipelineData::ListStream(stream, metadata) => {
            if let Some((
                &PathMember::Int {
                    val,
                    span: path_span,
                    ..
                },
                path,
            )) = cell_path.members.split_first()
            {
                let mut stream = stream.into_iter();
                let mut pre_elems = vec![];

                for idx in 0..=val {
                    if let Some(v) = stream.next() {
                        pre_elems.push(v);
                    } else if idx == 0 {
                        return Err(ShellError::AccessEmptyContent { span: path_span });
                    } else {
                        return Err(ShellError::AccessBeyondEnd {
                            max_idx: idx - 1,
                            span: path_span,
                        });
                    }
                }

                // cannot fail since loop above does at least one iteration or returns an error
                let value = pre_elems.last_mut().expect("one element");

                if let Value::Closure { val, .. } = replacement {
                    update_single_value_by_closure(
                        value,
                        ClosureEvalOnce::new(engine_state, stack, *val),
                        head,
                        path,
                        true,
                    )?;
                } else {
                    value.update_data_at_cell_path(path, replacement)?;
                }

                Ok(pre_elems
                    .into_iter()
                    .chain(stream)
                    .into_pipeline_data_with_metadata(
                        head,
                        engine_state.signals().clone(),
                        metadata,
                    ))
            } else if let Value::Closure { val, .. } = replacement {
                let mut closure = ClosureEval::new(engine_state, stack, *val);
                let stream = stream.map(move |mut value| {
                    let err = update_value_by_closure(
                        &mut value,
                        &mut closure,
                        head,
                        &cell_path.members,
                        false,
                    );

                    if let Err(e) = err {
                        Value::error(e, head)
                    } else {
                        value
                    }
                });

                Ok(PipelineData::ListStream(stream, metadata))
            } else {
                let stream = stream.map(move |mut value| {
                    if let Err(e) =
                        value.update_data_at_cell_path(&cell_path.members, replacement.clone())
                    {
                        Value::error(e, head)
                    } else {
                        value
                    }
                });

                Ok(PipelineData::ListStream(stream, metadata))
            }
        }
        PipelineData::Empty => Err(ShellError::IncompatiblePathAccess {
            type_name: "empty pipeline".to_string(),
            span: head,
        }),
        PipelineData::ByteStream(stream, ..) => Err(ShellError::IncompatiblePathAccess {
            type_name: stream.type_().describe().into(),
            span: head,
        }),
    }
}

fn update_value_by_closure(
    value: &mut Value,
    closure: &mut ClosureEval,
    span: Span,
    cell_path: &[PathMember],
    first_path_member_int: bool,
) -> Result<(), ShellError> {
    let value_at_path = value.follow_cell_path(cell_path)?;

    let arg = if first_path_member_int {
        value_at_path.as_ref()
    } else {
        &*value
    };

    let new_value = closure
        .add_arg(arg.clone())
        .run_with_input(value_at_path.into_owned().into_pipeline_data())?
        .into_value(span)?;

    value.update_data_at_cell_path(cell_path, new_value)
}

fn update_single_value_by_closure(
    value: &mut Value,
    closure: ClosureEvalOnce,
    span: Span,
    cell_path: &[PathMember],
    first_path_member_int: bool,
) -> Result<(), ShellError> {
    let value_at_path = value.follow_cell_path(cell_path)?;

    let arg = if first_path_member_int {
        value_at_path.as_ref()
    } else {
        &*value
    };

    let new_value = closure
        .add_arg(arg.clone())
        .run_with_input(value_at_path.into_owned().into_pipeline_data())?
        .into_value(span)?;

    value.update_data_at_cell_path(cell_path, new_value)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_examples() {
        use crate::test_examples;

        test_examples(Update {})
    }
}
