# Todo

For `utils`:

* [Buffers](https://github.com/wren-lang/wren/blob/main/src/vm/wren_utils.h#L16)
  * ByteBuffer
  * IntBuffer
  * StringBuffer
* [SymbolTable](https://github.com/wren-lang/wren/blob/main/src/vm/wren_utils.h#L71)
  * Use std HashMap

For `values`:

* Define the wren [types](https://github.com/wren-lang/wren/blob/a4ae90538445a4f88dc965e9f11c768ae903ff0d/src/vm/wren_value.h#L49)
  * [Object types](https://github.com/wren-lang/wren/blob/a4ae90538445a4f88dc965e9f11c768ae903ff0d/src/vm/wren_value.h#L90)
  * [Value types](https://github.com/wren-lang/wren/blob/a4ae90538445a4f88dc965e9f11c768ae903ff0d/src/vm/wren_value.h#L127)

* Buffers
  * ValueBuffer

For `vm`:

* [Lexer](https://github.com/wren-lang/wren/blob/main/src/vm/wren_compiler.c#L158)
  * [Methods](https://github.com/wren-lang/wren/blob/main/src/vm/wren_compiler.c#L639) for peeking and next
  * [Methods](https://github.com/wren-lang/wren/blob/main/src/vm/wren_compiler.c#L699) for skipping
* [Token](https://github.com/wren-lang/wren/blob/main/src/vm/wren_compiler.c#L53) for lexer
* [Keywords](https://github.com/wren-lang/wren/blob/main/src/vm/wren_compiler.c#L592)
* [Print errors](https://github.com/wren-lang/wren/blob/main/src/vm/wren_compiler.c#L420)
* Nothing for parser yet

For `core`:

* Used by vm parser, nothing needs to be done yet.

Other modules provide supportive functions.
