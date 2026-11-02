# SOM interface repository files (`.IR`)

SOM 2.1's interface repository (IR) keeps the IDL of SOM classes for
programs to look at while they run: the SOM compiler writes it (`sc -sir`,
its `emitir` emitter), and SOMIR.DLL's classes (Repository, InterfaceDef,
OperationDef, ...) read it. OS/2 Warp 4.52 has `SOM.IR` (SOM's own classes,
DSOM, the persistence and replication frameworks) and `WPSH.IR` (the
Workplace Shell) in `\OS2\ETC`; the `SOMIR` environment variable lists the
files a program's repository is made of, separated by semicolons (IBM's
default: `som.ir`). The last one in the list is the one updates go to.

This was worked out from IBM's SOMIR.DLL (Ghidra: the `*FromBuf`/`*InBuf`
helpers, Contained's, Container's and the definitions' `getDataFrom...` and
`putDataIn...` methods, the record allocator) and checked by reading all of
`SOM.IR` with `tools/scripts/irdump.py`; rs2's SOMIR.DLL (`userland/somir`)
reads it as described here.

All integers are **big-endian** 32-bit. A file is a heap of records; a
record is addressed by its byte offset in the file, which SOMIR calls the
record's **IID**. IID 0 means "none".

## Encodings

| Item | Encoding |
|------|----------|
| integer | 4 bytes, big-endian |
| string | its length *n* (without the NUL), then *n* bytes and a NUL; a missing (NULL) string is length -1 and a single NUL byte |
| string sequence | a count, then that many strings |
| TypeCode | see below |

### TypeCodes

A TypeCode is its kind (SOMTC's `TCKind`: tk_short 3, tk_long 4, ...,
tk_struct 16, tk_union 17, tk_enum 18, tk_string 19, tk_sequence 20,
tk_array 21, tk_pointer 101, tk_self 102, tk_foreign 103; -1 for no
TypeCode and nothing follows), its explicit alignment (0: the default),
two reserved integers (0), then by kind:

| Kind | Parameters |
|------|------------|
| tk_objref | the interface's id (string) |
| tk_struct | name, member count, then each member's name and TypeCode |
| tk_union | name, the switch's TypeCode, member count, then per member: a flag (1 a label follows, 2 the default case), the label (flag 1 only), the member's name and TypeCode |
| tk_enum | name, count, the names |
| tk_string | maximum length (0: unbounded) |
| tk_sequence, tk_array | element TypeCode, bound / length |
| tk_pointer | the TypeCode pointed to |
| tk_self | the name of the structure or union it refers to |
| tk_foreign | type name, implementation context, instance size |
| others | nothing |

## The header (IID 0, 32 bytes)

| Offset | Contents |
|--------|----------|
| 0 | magic `0x05179979` |
| 4 | 200 (the format version) |
| 8 | IID of the top level's contents record |
| 12 | IID of the first free block (0: none) |
| 16-31 | 0 |

## Records

Every record starts with its size (the space it occupies) and a type
word:

| Type | Record |
|------|--------|
| a class code | a definition's **fixed record** |
| 8 | a container's **contents** |
| 9 | a **reference list** |
| 10 | a **free block**: size, 10, the next free block's IID |
| 11 | a definition's **variable record** |
| -11 | space just allocated, not written yet |

Class codes (IBM's `FUN_0001ca74`): 0 ConstantDef, 1 TypeDef, 2
ExceptionDef, 3 InterfaceDef, 4 ModuleDef, 5 AttributeDef, 6 OperationDef,
7 ParameterDef.

### Fixed records

After the size and the class code, every definition has Contained's part:

| Field | |
|-------|-|
| string | the name |
| integer | 0 |
| integer | the IID of the definition it is in (0: the top level) |
| integer | the IID of its reference list (0: none) |
| integer | the IID of its variable record (0: none) |

then its class's part:

| Class | Fixed part |
|-------|------------|
| AttributeDef | mode (1 NORMAL, 2 READONLY) |
| ParameterDef | mode (1 IN, 2 OUT, 3 INOUT) |
| InterfaceDef, ModuleDef | the IID of its contents record |
| OperationDef | the IID of its contents record (its parameters), 0, mode (1 NORMAL, 2 ONEWAY) |
| TypeDef, ConstantDef, ExceptionDef | nothing |

A definition's repository id is not stored: it is the enclosing
definition's id, `::` and the name (`::SOMObject::somInit`; `::name` at the
top level).

### Variable records

Size, 11, the IID of the fixed record it belongs to, 0, then the
modifiers (Contained's `somModifiers`, the IDL's implementation section):
a count, then each modifier's name and value (the value NULL for a
modifier without one, such as `init`), then the class's part:

| Class | Variable part |
|-------|---------------|
| AttributeDef, ParameterDef, TypeDef, ExceptionDef | TypeCode |
| ConstantDef | TypeCode, the value as text (`"2.1"`, `256`) |
| InterfaceDef | base interfaces (string sequence of ids), the instance data's TypeCode |
| OperationDef | the result's TypeCode, contexts (string sequence), the exceptions it raises (string sequence of ids) |

Interfaces' modifiers hold what SOM's runtime looks up: `dllname`
(SOMClassMgr finds a class's DLL with Repository's `lookup_modifier`),
`majorversion`, `minorversion`, `releaseorder`, `filestem`, `callstyle`,
`override`, and `file`/`line` (where the IDL was).

### Contents records

Size, 8, the number of entries, then per entry: the definition's IID, its
class code and its name. Names are in IDL order.

### Reference lists

Size, 9, a count, then IIDs: the definitions that refer to this one (an
interface's derived interfaces).

## Updates

IBM's SOMIR allocates records from the free list (first fit, splitting
larger blocks and merging neighbours, `irMalloc`/`irFree`) and rewrites a
definition's records in place when they still fit. rs2's SOMIR reads
repositories only.
