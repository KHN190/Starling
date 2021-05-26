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
  current_char_i: usize,

  // The 1-based line number of [currentChar].
  current_line: usize,

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
  parens: [usize; MAX_INTERPOLATION_NESTING],
  num_parens: usize,

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

fn is_name(c: char) -> bool {
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_';
}

fn is_digit(c: char) -> bool {
    return c >= '0' && c <= '9';
}

impl Parser {
    // @todo
    //   configurable with args
    fn print_error(&self, line: usize, label: &str, format: &str) {
        unimplemented!()
    }

    // @todo
    //   configurable with args
    // Outputs a lexical error.
    fn lex_error(&self, format: &str) {
        self.print_error(self.current_line, "Error", format);
    }

    fn peek_char(&self) -> char {
        self.source.chars().nth(self.current_char_i).unwrap_or('\0')
    }

    fn peek_next_char(&self) -> char {
        self.source.chars().nth(self.current_char_i + 1).unwrap_or('\0')
    }

    fn next_char(&mut self) -> char {
        let c = self.peek_char();
        self.current_char_i += 1;
        if c == '\n' { self.current_line += 1; }
        c
    }

    fn match_char(&mut self, c: char) -> bool {
        if self.peek_char() != c { return false; }
        self.next_char();
        true
    }

    // Sets the parser's current token to the given [type] and current character
    // range.
    fn make_token(&self) { unimplemented!() }

    // If the current character is [c], then consumes it and makes a token of type
    // [two]. Otherwise makes a token of type [one].
    fn two_char_token(&self, c: char, two: Token, one: Token) { unimplemented!() }

    // Skips the rest of the current line.
    fn skip_line_comment(&mut self) {
        while self.peek_char() != '\n' && self.peek_char() != '\0' {
            self.next_char();
        }
    }

    // Skips the rest of a block comment.
    fn skip_block_comment(&mut self) {
        let mut nesting: usize = 1;
        while nesting > 0 {
            if self.peek_char() == '\0' {
                self.lex_error("Unterminated block comment.");
                return;
            }

            if self.peek_char() == '/' && self.peek_next_char() == '*' {
                self.next_char();
                self.next_char();
                nesting += 1;
                continue;
            }

            if self.peek_char() == '*' && self.peek_next_char() == '/' {
                self.next_char();
                self.next_char();
                nesting -= 1;
                continue;
            }
            // Regular comment character.
            self.next_char();
        }
    }

    // Reads the next character, which should be a hex digit (0-9, a-f, or A-F) and
    // returns its numeric value. If the character isn't a hex digit, returns -1.
    fn read_hex_digit(&mut self) -> i32 {
        let c = self.next_char();
        if c >= '0' && c <= '9' { return (c as i32) - ('0' as i32); }
        if c >= 'a' && c <= 'f' { return (c as i32) - ('a' as i32) + 10; }
        if c >= 'A' && c <= 'F' { return (c as i32) - ('A' as i32) + 10; }

        // Don't consume it if it isn't expected. Keeps us from reading past the end
        // of an unterminated string.
        self.current_char_i -= 1;

        -1
    }

    // Parses the numeric value of the current token.
    fn make_number(&self, is_hex: bool) { unimplemented!() }

    // Finishes lexing a hexadecimal number literal.
    fn read_hex_number(&mut self) {
        // Skip past the `x` used to denote a hexadecimal literal.
        self.next_char();
        // Iterate over all the valid hexadecimal digits found.
        while self.read_hex_digit() != -1 { continue; }

        self.make_number(true);
    }
}
