use crate::{
    Token, TokenContents,
    lex::{LexState, is_assignment_operator, lex, lex_n_tokens, lex_signature},
    lite_parser::{LiteCommand, LitePipeline, LiteRedirection, LiteRedirectionTarget, lite_parse},
    parse_keywords::*,
    parse_patterns::parse_pattern,
    parse_shape_specs::{parse_completer, parse_shape_name, parse_type},
    type_check::{self, check_range_types, math_result_type, type_compatible},
};
use itertools::Itertools;
use log::trace;
use nu_engine::DIR_VAR_PARSER_INFO;
use nu_protocol::{
    BlockId, DeclId, DidYouMean, ENV_VARIABLE_ID, FilesizeUnit, Flag, IN_VARIABLE_ID, ParseError,
    PositionalArg, ShellError, Signature, Span, Spanned, SyntaxShape, Type, Value, VarId, ast::*,
    casing::Casing, did_you_mean, engine::StateWorkingSet, eval_const::eval_constant,
};
use std::{
    collections::{HashMap, HashSet},
    str,
    sync::Arc,
};

fn parse_overlay_predecl
