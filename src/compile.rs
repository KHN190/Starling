use crate::common::MAX_VARIABLE_NAME;
use crate::vm::WrenVM;
use crate::value::*;

// This is written in bottom-up order, so the tokenization comes first, then
// parsing/code generation. This minimizes the number of explicit forward
// declarations needed.

// The maximum number of local (i.e. not module level) variables that can be
// declared in a single function, method, or chunk of top level code. This is
// the maximum number of variables in scope at one time, and spans block scopes.
//
// Note that this limitation is also explicit in the bytecode. Since
// `CODE_LOAD_LOCAL` and `CODE_STORE_LOCAL` use a single argument byte to
// identify the local, only 256 can be in scope at one time.
pub(crate) const MAX_LOCALS: usize = 256;

// The maximum number of upvalues (i.e. variables from enclosing functions)
// that a function can close over.
pub(crate) const MAX_UPVALUES: usize = 256;

// The maximum number of distinct constants that a function can contain. This
// value is explicit in the bytecode since `CODE_CONSTANT` only takes a single
// two-byte argument.
pub(crate) const MAX_CONSTANTS: i32 = 1 << 16;

// The maximum distance a CODE_JUMP or CODE_JUMP_IF instruction can move the
// instruction pointer.
pub(crate) const MAX_JUMP: i32 = 1 << 16;

// The maximum depth that interpolation can nest. For example, this string has
// three levels:
//
//      "outside %(one + "%(two + "%(three)")")"
pub(crate) const MAX_INTERPOLATION_NESTING: usize = 8;

// The buffer size used to format a compile error message, excluding the header
// with the module name and error location. Using a hardcoded buffer for this
// is kind of hairy, but fortunately we can control what the longest possible
// message is and handle that. Ideally, we'd use `snprintf()`, but that's not
// available in standard C++98.
pub(crate) const ERROR_MESSAGE_SIZE: i32 = 80 + MAX_VARIABLE_NAME + 15;

#[allow(dead_code, non_camel_case_types)]
enum TokenType {
  LEFT_PAREN,
  RIGHT_PAREN,
  LEFT_BRACKET,
  RIGHT_BRACKET,
  LEFT_BRACE,
  RIGHT_BRACE,
  COLON,
  DOT,
  DOTDOT,
  DOTDOTDOT,
  COMMA,
  STAR,
  SLASH,
  PERCENT,
  HASH,
  PLUS,
  MINUS,
  LTLT,
  GTGT,
  PIPE,
  PIPEPIPE,
  CARET,
  AMP,
  AMPAMP,
  BANG,
  TILDE,
  QUESTION,
  EQ,
  LT,
  GT,
  LTEQ,
  GTEQ,
  EQEQ,
  BANGEQ,

  BREAK,
  CONTINUE,
  CLASS,
  CONSTRUCT,
  ELSE,
  FALSE,
  FOR,
  FOREIGN,
  IF,
  IMPORT,
  AS,
  IN,
  IS,
  NULL,
  RETURN,
  STATIC,
  SUPER,
  THIS,
  TRUE,
  VAR,
  WHILE,

  FIELD,
  STATIC_FIELD,
  NAME,
  NUMBER,

  // A string literal without any interpolation, or the last section of a
  // string following the last interpolated expression.
  STRING,

  // A portion of a string literal preceding an interpolated expression. This
  // string:
  //
  //     "a %(b) c %(d) e"
  //
  // is tokenized to:
  //
  //     INTERPOLATION "a "
  //     NAME          b
  //     INTERPOLATION " c "
  //     NAME          d
  //     STRING        " e"
  INTERPOLATION,

  LINE,

  ERROR,
  EOF
}

struct Keyword {
    identifier: &'static str,
    token_type: TokenType,
}

macro_rules! define_keyword {
    ($id:expr, $ty:tt) => {
        Keyword { identifier: $id, token_type: TokenType::$ty }
    };
}

// The table of reserved words and their associated token types.
#[allow(dead_code)]
static KEYWORDS: &'static [Keyword] = &[
    define_keyword!("break", BREAK),
    define_keyword!("continue", CONTINUE),
    define_keyword!("class", CLASS),
    define_keyword!("construct", CONSTRUCT),
    define_keyword!("else", ELSE),
    define_keyword!("false", FALSE),
    define_keyword!("for", FOR),
    define_keyword!("foreign", FOREIGN),
    define_keyword!("if", IF),
    define_keyword!("import", IMPORT),
    define_keyword!("as", AS),
    define_keyword!("in", IN),
    define_keyword!("is", IS),
    define_keyword!("null", NULL),
    define_keyword!("return", RETURN),
    define_keyword!("static", STATIC),
    define_keyword!("super", SUPER),
    define_keyword!("this", THIS),
    define_keyword!("true", TRUE),
    define_keyword!("var", VAR),
    define_keyword!("while", WHILE),
    define_keyword!("", EOF), // @todo ??
];

struct Token
{
  ty: TokenType,

  // The beginning of the token, pointing directly into the source.
  start: String, // @todo ??

  // The length of the token in characters.
  length: i32,

  // The 1-based line where the token appears.
  line: i32,

  // The parsed value if the token is a literal.
  value: Value,
}

struct Parser
{
  vm: WrenVM,

  // The module being parsed.
  module: ObjModule,

  // The source code being parsed.
  source: String,

  // The beginning of the currently-being-lexed token in [source].
  token_start: String,

  // The current character being lexed in [source].
  current_char: String,

  // The 1-based line number of [currentChar].
  current_line: i32,

  // The upcoming token.
  next: Token,

  // The most recently lexed token.
  current: Token,

  // The most recently consumed/advanced token.
  previous: Token,

  // Tracks the lexing state when tokenizing interpolated strings.
  //
  // Interpolated strings make the lexer not strictly regular: we don't know
  // whether a ")" should be treated as a RIGHT_PAREN token or as ending an
  // interpolated expression unless we know whether we are inside a string
  // interpolation and how many unmatched "(" there are. This is particularly
  // complex because interpolation can nest:
  //
  //     " %( " %( inner ) " ) "
  //
  // This tracks that state. The parser maintains a stack of ints, one for each
  // level of current interpolation nesting. Each value is the number of
  // unmatched "(" that are waiting to be closed.
  parens: [i32; MAX_INTERPOLATION_NESTING],
  num_parens: i32,

  // Whether compile errors should be printed to stderr or discarded.
  print_errors: bool,

  // If a syntax or compile error has occurred.
  has_error: bool,
}
//
// static void printError(Parser* parser, int line, const char* label,
//                        const char* format, va_list args)
// {
//   parser->hasError = true;
//   if (!parser->printErrors) return;
//
//   // Only report errors if there is a WrenErrorFn to handle them.
//   if (parser->vm->config.errorFn == NULL) return;
//
//   // Format the label and message.
//   char message[ERROR_MESSAGE_SIZE];
//   int length = sprintf(message, "%s: ", label);
//   length += vsprintf(message + length, format, args);
//   ASSERT(length < ERROR_MESSAGE_SIZE, "Error should not exceed buffer.");
//
//   ObjString* module = parser->module->name;
//   const char* module_name = module ? module->value : "<unknown>";
//
//   parser->vm->config.errorFn(parser->vm, WREN_ERROR_COMPILE,
//                              module_name, line, message);
// }
//
impl Parser {
    pub fn print_errors(self: Self, line: i32, label: &str, format: &str, ) {

    }
}

// // Outputs a lexical error.
// static void lexError(Parser* parser, const char* format, ...)
// {
//   va_list args;
//   va_start(args, format);
//   printError(parser, parser->currentLine, "Error", format, args);
//   va_end(args);
// }
