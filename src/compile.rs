use std::rc::Rc;
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

#[allow(dead_code)]
enum TokenType {
  TOKEN_LEFT_PAREN,
  TOKEN_RIGHT_PAREN,
  TOKEN_LEFT_BRACKET,
  TOKEN_RIGHT_BRACKET,
  TOKEN_LEFT_BRACE,
  TOKEN_RIGHT_BRACE,
  TOKEN_COLON,
  TOKEN_DOT,
  TOKEN_DOTDOT,
  TOKEN_DOTDOTDOT,
  TOKEN_COMMA,
  TOKEN_STAR,
  TOKEN_SLASH,
  TOKEN_PERCENT,
  TOKEN_HASH,
  TOKEN_PLUS,
  TOKEN_MINUS,
  TOKEN_LTLT,
  TOKEN_GTGT,
  TOKEN_PIPE,
  TOKEN_PIPEPIPE,
  TOKEN_CARET,
  TOKEN_AMP,
  TOKEN_AMPAMP,
  TOKEN_BANG,
  TOKEN_TILDE,
  TOKEN_QUESTION,
  TOKEN_EQ,
  TOKEN_LT,
  TOKEN_GT,
  TOKEN_LTEQ,
  TOKEN_GTEQ,
  TOKEN_EQEQ,
  TOKEN_BANGEQ,

  TOKEN_BREAK,
  TOKEN_CONTINUE,
  TOKEN_CLASS,
  TOKEN_CONSTRUCT,
  TOKEN_ELSE,
  TOKEN_FALSE,
  TOKEN_FOR,
  TOKEN_FOREIGN,
  TOKEN_IF,
  TOKEN_IMPORT,
  TOKEN_AS,
  TOKEN_IN,
  TOKEN_IS,
  TOKEN_NULL,
  TOKEN_RETURN,
  TOKEN_STATIC,
  TOKEN_SUPER,
  TOKEN_THIS,
  TOKEN_TRUE,
  TOKEN_VAR,
  TOKEN_WHILE,

  TOKEN_FIELD,
  TOKEN_STATIC_FIELD,
  TOKEN_NAME,
  TOKEN_NUMBER,

  // A string literal without any interpolation, or the last section of a
  // string following the last interpolated expression.
  TOKEN_STRING,

  // A portion of a string literal preceding an interpolated expression. This
  // string:
  //
  //     "a %(b) c %(d) e"
  //
  // is tokenized to:
  //
  //     TOKEN_INTERPOLATION "a "
  //     TOKEN_NAME          b
  //     TOKEN_INTERPOLATION " c "
  //     TOKEN_NAME          d
  //     TOKEN_STRING        " e"
  TOKEN_INTERPOLATION,

  TOKEN_LINE,

  TOKEN_ERROR,
  TOKEN_EOF
}

struct Token
{
  ty: TokenType,

  // The beginning of the token, pointing directly into the source.
  start: String,

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

struct Local
{
  // The name of the local variable. This points directly into the original
  // source code string.
  name: String,

  // The length of the local variable's name.
  length: i32,

  // The depth in the scope chain that this variable was declared at. Zero is
  // the outermost scope--parameters for a method, or the first local block in
  // top level code. One is the scope within that, etc.
  depth: i32,

  // If this local variable is being used as an upvalue.
  is_upvalue: bool,
}

struct CompilerUpvalue
{
  // True if this upvalue is capturing a local variable from the enclosing
  // function. False if it's capturing an upvalue.
  is_local: bool,

  // The index of the local or upvalue being captured in the enclosing function.
  index: i32,
}

// Bookkeeping information for the current loop being compiled.
struct SLoop
{
  // Index of the instruction that the loop should jump back to.
  start: i32,

  // Index of the argument for the CODE_JUMP_IF instruction used to exit the
  // loop. Stored so we can patch it once we know where the loop ends.
  exit_jump: i32,

  // Index of the first instruction of the body of the loop.
  body: i32,

  // Depth of the scope(s) that need to be exited if a break is hit inside the
  // loop.
  scope_depth: i32,

  // The loop enclosing this one, or NULL if this is the outermost loop.
  enclosing: Rc<SLoop>,
}

// The different signature syntaxes for different kinds of methods.
#[allow(dead_code)]
enum SignatureType
{
  // A name followed by a (possibly empty) parenthesized parameter list. Also
  // used for binary operators.
  SIG_METHOD,

  // Just a name. Also used for unary operators.
  SIG_GETTER,

  // A name followed by "=".
  SIG_SETTER,

  // A square bracketed parameter list.
  SIG_SUBSCRIPT,

  // A square bracketed parameter list followed by "=".
  SIG_SUBSCRIPT_SETTER,

  // A constructor initializer function. This has a distinct signature to
  // prevent it from being invoked directly outside of the constructor on the
  // metaclass.
  SIG_INITIALIZER
}

struct Signature
{
  name: String,
  length: i32,
  ty: SignatureType,
  arity: i32,
}

// Bookkeeping information for compiling a class definition.
struct ClassInfo
{
  // The name of the class.
  name: ObjString,

  // Attributes for the class itself
  class_attributes: ObjMap,

  // Attributes for methods in this class
  method_attributes: ObjMap,

  // Symbol table for the fields of the class.
  fields: SymbolTable<String, String>,

  // Symbols for the methods defined by the class. Used to detect duplicate
  // method definitions.
  methods: Buffer<i32>,
  static_methods: Buffer<i32>,

  // True if the class being compiled is a foreign class.
  is_foreign: bool,

  // True if the current method being compiled is static.
  in_static: bool,

  // The signature of the method being compiled.
  signature: Signature,
}

struct SCompiler
{
  parser: Parser,

  // The compiler for the function enclosing this one, or NULL if it's the
  // top level.
  parent: Rc<SCompiler>,

  // The currently in scope local variables.
  locals: [Local; MAX_LOCALS],

  // The number of local variables currently in scope.
  num_locals: i32,

  // The upvalues that this function has captured from outer scopes. The count
  // of them is stored in [numUpvalues].
  upvalues: [CompilerUpvalue; MAX_UPVALUES],

  // The current level of block scope nesting, where zero is no nesting. A -1
  // here means top-level code is being compiled and there is no block scope
  // in effect at all. Any variables declared will be module-level.
  scope_depth: i32,

  // The current number of slots (locals and temporaries) in use.
  //
  // We use this and maxSlots to track the maximum number of additional slots
  // a function may need while executing. When the function is called, the
  // fiber will check to ensure its stack has enough room to cover that worst
  // case and grow the stack if needed.
  //
  // This value here doesn't include parameters to the function. Since those
  // are already pushed onto the stack by the caller and tracked there, we
  // don't need to double count them here.
  num_slots: i32,

  // The current innermost loop being compiled, or NULL if not in a loop.
  sloop: SLoop,

  // If this is a compiler for a method, keeps track of the class enclosing it.
  enclosing_class: ClassInfo,

  // The function being compiled.
  func: ObjFn,

  // The constants for the function being compiled.
  constants: ObjMap,

  // Whether or not the compiler is for a constructor initializer
  is_initializer: bool,

  // The number of attributes seen while parsing.
  // We track this separately as compile time attributes
  // are not stored, so we can't rely on attributes->count
  // to enforce an error message when attributes are used
  // anywhere other than methods or classes.
  num_attributes: i32,

  // Attributes for the next class or method.
  attributes: ObjMap,
}

// Describes where a variable is declared.
#[allow(dead_code)]
enum Scope
{
  // A local variable in the current function.
  SCOPE_LOCAL,

  // A local variable declared in an enclosing function.
  SCOPE_UPVALUE,

  // A top-level module variable.
  SCOPE_MODULE
}

// A reference to a variable and the scope where it is defined. This contains
// enough information to emit correct code to load or store the variable.
struct Variable
{
  // The stack slot, upvalue slot, or module symbol defining the variable.
  index: i32,

  // Where the variable is declared.
  scope: Scope,
}
