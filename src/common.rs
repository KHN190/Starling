// These flags are useful for debugging and hacking on Wren itself. They are not
// intended to be used for production code. They default to off.

// Set this to true to stress test the GC. It will perform a collection before
// every allocation. This is useful to ensure that memory is always correctly
// reachable.
pub(crate) const WREN_DEBUG_GC_STRESS: i32 = 0;

// Set this to true to log memory operations as they occur.
pub(crate) const WREN_DEBUG_TRACE_MEMORY: i32 = 0;

// Set this to true to log garbage collections as they occur.
pub(crate) const WREN_DEBUG_TRACE_GC: i32 = 0;

// Set this to true to print out the compiled bytecode of each function.
pub(crate) const WREN_DEBUG_DUMP_COMPILED_CODE: i32 = 0;

// Set this to trace each instruction as it's executed.
pub(crate) const WREN_DEBUG_TRACE_INSTRUCTIONS: i32 = 0;

// The maximum number of module-level variables that may be defined at one time.
// This limitation comes from the 16 bits used for the arguments to
// `CODE_LOAD_MODULE_VAR` and `CODE_STORE_MODULE_VAR`.
pub(crate) const MAX_MODULE_VARS: i32 = 65536;

// The maximum number of arguments that can be passed to a method. Note that
// this limitation is hardcoded in other places in the VM, in particular, the
// `CODE_CALL_XX` instructions assume a certain maximum number.
pub(crate) const MAX_PARAMETERS: i32 = 16;

// The maximum name of a method, not including the signature. This is an
// arbitrary but enforced maximum just so we know how long the method name
// strings need to be in the parser.
pub(crate) const MAX_METHOD_NAME: i32 = 64;

// The maximum length of a method signature. Signatures look like:
//
//     foo        // Getter.
//     foo()      // No-argument method.
//     foo(_)     // One-argument method.
//     foo(_,_)   // Two-argument method.
//     init foo() // Constructor initializer.
//
// The maximum signature length takes into account the longest method name, the
// maximum number of parameters with separators between them, "init ", and "()".
pub(crate) const MAX_METHOD_SIGNATURE: i32 = MAX_METHOD_NAME + (MAX_PARAMETERS * 2) + 6;

// The maximum length of an identifier. The only real reason for this limitation
// is so that error messages mentioning variables can be stack allocated.
pub(crate) const MAX_VARIABLE_NAME: i32 = 64;

// The maximum number of fields a class can have, including inherited fields.
// This is explicit in the bytecode since `CODE_CLASS` and `CODE_SUBCLASS` take
// a single byte for the number of fields. Note that it's 255 and not 256
// because creating a class takes the *number* of fields, not the *highest
// field index*.
pub(crate) const MAX_FIELDS: i32 = 255;

// Use the VM's allocator to allocate an object of [type].
// ALLOCATE(vm, type)
//     ((type*)wrenReallocate(vm, NULL, 0, sizeof(type)))

// Use the VM's allocator to allocate an object of [mainType] containing a
// flexible array of [count] objects of [arrayType].
// ALLOCATE_FLEX(vm, mainType, arrayType, count)
//     ((mainType*)wrenReallocate(vm, NULL, 0,
//         sizeof(mainType) + sizeof(arrayType) * (count)))

// Use the VM's allocator to allocate an array of [count] elements of [type].
// ALLOCATE_ARRAY(vm, type, count)
//     ((type*)wrenReallocate(vm, NULL, 0, sizeof(type) * (count)))

// Use the VM's allocator to free the previously allocated memory at [pointer].
// DEALLOCATE(vm, pointer) wrenReallocate(vm, pointer, 0, 0)

// Assertions are used to validate program invariants. They indicate things the
// program expects to be true about its internal state during execution. If an
// assertion fails, there is a bug in Wren.
//
// Assertions add significant overhead, so are only enabled in debug builds.
//
// We use Rust built-in macros here.

// assert!()
// unreachable!()
