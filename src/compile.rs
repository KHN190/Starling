use crate::common::MAX_VARIABLE_NAME;
use crate::value::*;
use crate::vm::WrenVM;

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
    EOF,
}

struct Keyword {
    identifier: &'static str,
    token_type: TokenType,
}

impl Keyword {
    pub fn len(&self) -> usize {
        self.identifier.len()
    }
}

macro_rules! define_keyword {
    ($id:expr, $ty:tt) => {
        Keyword {
            identifier: $id,
            token_type: TokenType::$ty,
        }
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

struct Token {
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

struct Parser {
    vm: WrenVM,

    // The module being parsed.
    module: ObjModule,

    // The source code being parsed.
    source: String,

    // The beginning of the currently-being-lexed token in [source].
    token_start: usize,

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

    fn read_token_str(&self, length: usize) -> &str {
        &self.source[self.token_start..self.token_start + length]
    }

    fn peek_char(&self) -> char {
        self.source.chars().nth(self.current_char_i).unwrap_or('\0')
    }

    fn peek_next_char(&self) -> char {
        self.source
            .chars()
            .nth(self.current_char_i + 1)
            .unwrap_or('\0')
    }

    fn next_char(&mut self) -> char {
        let c = self.peek_char();
        self.current_char_i += 1;
        if c == '\n' {
            self.current_line += 1;
        }
        c
    }

    fn match_char(&mut self, c: char) -> bool {
        if self.peek_char() != c {
            return false;
        }
        self.next_char();
        true
    }

    // Sets the parser's current token to the given [type] and current character
    // range.
    fn make_token(&self, ty: TokenType) {
        unimplemented!()
    }

    // If the current character is [c], then consumes it and makes a token of type
    // [two]. Otherwise makes a token of type [one].
    fn two_char_token(&self, c: char, two: Token, one: Token) {
        unimplemented!()
    }

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
        if c >= '0' && c <= '9' {
            return (c as i32) - ('0' as i32);
        }
        if c >= 'a' && c <= 'f' {
            return (c as i32) - ('a' as i32) + 10;
        }
        if c >= 'A' && c <= 'F' {
            return (c as i32) - ('A' as i32) + 10;
        }

        // Don't consume it if it isn't expected. Keeps us from reading past the end
        // of an unterminated string.
        self.current_char_i -= 1;

        -1
    }

    // Parses the numeric value of the current token.
    fn make_number(&self, is_hex: bool) {
        unimplemented!()
    }

    // Finishes lexing a hexadecimal number literal.
    fn read_hex_number(&mut self) {
        // Skip past the `x` used to denote a hexadecimal literal.
        self.next_char();
        // Iterate over all the valid hexadecimal digits found.
        while self.read_hex_digit() != -1 {
            continue;
        }
        self.make_number(true);
    }

    // Finishes lexing a number literal.
    fn read_number(&mut self) {
        while is_digit(self.peek_char()) {
            self.next_char();
        }

        // See if it has a floating point. Make sure there is a digit after the "."
        // so we don't get confused by method calls on number literals.
        if self.peek_char() == '.' && is_digit(self.peek_next_char()) {
            self.next_char();
            while is_digit(self.peek_char()) {
                self.next_char();
            }
        }

        // See if the number is in scientific notation.
        if self.match_char('e') || self.match_char('E') {
            // Allow a single positive/negative exponent symbol.
            if !self.match_char('+') {
                self.match_char('-');
            }
            if !is_digit(self.peek_char()) {
                self.lex_error("Unterminated scientific notation.");
            }
            while is_digit(self.peek_char()) {
                self.next_char();
            }
        }
        self.make_number(false);
    }

    // Finishes lexing an identifier. Handles reserved words.
    fn read_name(&mut self, ty: &TokenType, first_char: char) {
        let mut buffer = vec![];
        buffer.push(first_char);

        while is_name(self.peek_char()) || is_digit(self.peek_char()) {
            buffer.push(self.next_char());
        }
        // Update the type if it's a keyword.
        let mut token_ty = ty.clone();
        let length = self.current_char_i - self.token_start;
        for kw in KEYWORDS {
            if length == kw.len() && self.read_token_str(length) == kw.identifier {
                token_ty = &kw.token_type;
            }
        }

        unimplemented!();
        //   parser->next.value = wrenNewStringLength(parser->vm,
        //                                             (char*)string.data, string.count);
        //
        //   wrenByteBufferClear(parser->vm, &string);
        //   makeToken(parser, type);
        // }
    }

    // Reads [digits] hex digits in a string literal and returns their number value.
    fn read_hex_escape(&self, digits: i32, description: &str) {
        unimplemented!();
    }

    // Reads a hex digit Unicode escape sequence in a string literal.
    fn read_unicode_escape(&self, byte_buffer: &[i32], length: usize) {
        unimplemented!();
    }

    fn read_raw_string(&mut self) {
        let mut string: Vec<char> = vec![];
        let mut ty = TokenType::STRING;

        //consume the second and third "
        self.next_char();
        self.next_char();

        let mut skip_start: i32 = 0;
        let mut first_new_line: i32 = -1;

        let mut skip_end: i32 = -1;
        let mut last_new_line: i32 = -1;

        loop {
            let c = self.next_char();
            let c1 = self.peek_char();
            let c2 = self.peek_next_char();

            if c == '"' && c1 == '"' && c2 == '"' {
                break;
            }

            match c {
                '\r' => {
                    continue;
                }
                '\n' => {
                    last_new_line = string.len() as i32;
                    skip_end = last_new_line;
                    if first_new_line == -1 {
                        first_new_line = string.len() as i32
                    }
                }
                _ => {}
            }

            let is_whitespace = c == ' ' || c == '\t';
            if c == '\n' || is_whitespace {
                skip_end = 1;
            }

            // If we haven't seen a newline or other character yet,
            // and still seeing whitespace, count the characters
            // as skippable till we know otherwise
            let skippable = skip_start != -1 && is_whitespace && first_new_line == -1;
            if skippable {
                skip_start = string.len() as i32 + 1;
            }

            // We've counted leading whitespace till we hit something else,
            // but it's not a newline, so we reset skipStart since we need these characters
            // if (firstNewline == -1 && !isWhitespace && c != '\n') skipStart = -1;
            if first_new_line == -1 && !is_whitespace && c != '\n' {
                skip_start = -1;
            }

            if c == '\0' || c1 == '\0' || c2 == '\0' {
                self.lex_error("Unterminated raw string.");
                // Don't consume it if it isn't expected. Keeps us from reading past the
                // end of an unterminated string.
                self.current_char_i -= 1;
                break;
            }

            string.push(c);
        }

        // consume the second and third "
        self.next_char();
        self.next_char();

        let mut offset: i32 = 0;
        let mut count: i32 = string.len() as i32;

        if first_new_line != -1 && skip_start == first_new_line {
            offset = first_new_line + 1;
        }
        if last_new_line != -1 && skip_end == last_new_line {
            count = last_new_line;
        }
        if offset > count {
            count = 0;
        } else {
            count -= offset;
        }

        // @todo!()
        // self.next.value = wren_new_string_length(string, offset, count);

        self.make_token(ty);
    }
}
