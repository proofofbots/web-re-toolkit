# Finding things again after a rebuild

A protection script is rebuilt often. Identifiers are regenerated, the opcode table is permuted, output slots are shuffled, and the whole bundle is minified from scratch. Anything that locates code by matching the text of that build stops working the next time the vendor ships, and it stops working silently: a regex that no longer matches looks exactly like a target that no longer has the function.

The toolkit's answer is to identify by structure and behaviour, resolve that identity against one specific build, write the result to a lock file, and diff the next build against it. When something moves you get told what moved and how far, instead of an empty match.

## Shape, not text

`wre-ident` parses the script and rebuilds every top level function as two token streams.

The **skeleton** is pure structure. Node kinds and nothing else: `function if for binary member call return`. Identifiers are dropped entirely, literals become a type tag.

The **normalised text** is the skeleton with the parts worth keeping: numeric literal values, short string literals, operators, and property names. Property names survive because they are browser API names rather than generated ones, and a magic constant like `2166136261` survives because it is the whole point of the function that carries it.

Both streams are name blind, so renaming every binding in a build changes neither. A pattern written against the normalised text is doing the same job a regex over the source was doing, except it cannot be broken by a rename, by minifier spacing, or by statement reordering that the parser normalises away.

```
wre locate collect.js --target acme
```

## Evidence, not a single pattern

A role is not matched, it is scored. Each rule lists clues, each clue carries a weight, and a candidate's score is the weight it earned over the weight on offer. Clues that must hold are marked `required` and remove a candidate outright when they fail.

| clue | what it asks |
| --- | --- |
| `shape-text` | a pattern over the normalised, name blind text |
| `skeleton-hash` | the exact structural hash of a known build |
| `constants` | one of these numeric literals appears |
| `strings` | one of these string literals appears |
| `properties` | every one of these property names is reached |
| `object-keys` | some object literal in the body carries these keys |
| `arity` | the function takes this many parameters |
| `size`, `loops` | crude structural bounds, useful as tie breakers |
| `calls`, `called-by` | call graph adjacency to a role already resolved |
| `behaves` | call it for real and check what it returns |

`calls` and `called-by` are resolved in dependency order, so a rule can say "the function that calls whatever filled the hash role" without knowing either name. That is often the cheapest way to pin a wrapper whose own body is unremarkable.

`behaves` is the strongest clue and the only one that cannot be fooled by a body that merely looks right. It is evaluated through an `Oracle` the caller supplies, which is what lets `wre-ident` stay free of any engine dependency while the CLI wires it to a real V8 realm.

Two candidates that score within the rule's margin are reported as ambiguous rather than resolved. Guessing between them is how a toolkit ends up quietly bound to the wrong function.

## Locking a build

`--lock` writes what was resolved, against the digest of the script it was resolved from:

```
wre locate collect.js --target acme --lock targets/acme.lock
```

Each role records its binding, the structural hashes, a bottom-k signature of its 3-grams, and the evidence that justified it. The manifest holds the intent, the lock holds the answer for one build, and the two are separate on purpose.

## Reading the next build

```
wre drift targets/acme.lock collect-new.js
```

Every locked role comes back as one of four states.

- **Intact.** The normalised text hashes the same and the name is the same.
- **Renamed.** The text hashes the same under a different name. Nothing to do but re-lock.
- **Edited.** No exact match, but a function shares enough structure. The report gives the percentage so you can judge whether to re-read it.
- **Lost.** Nothing is close enough. `wre drift` exits non-zero, because this is the case that needs a person.

To compare whole builds rather than locked roles:

```
wre builds collect-old.js collect-new.js
```

Functions are paired by exact normalised hash first, then greedily by structural similarity, and each function is used at most once. What is left over is reported as gone or new.

## Why similarity counts repeats

Similarity is the Jaccard overlap of 3-grams over the skeleton, and the grams are a **multiset**. Comparing gram sets instead of bags looks equivalent and is not: protection code repeats the same short structural patterns constantly, so an edit that adds `member call` to a loop body frequently introduces only grams that already appear elsewhere. As sets the two functions then score exactly 1.0 and the edit vanishes. Counting multiplicity is what makes an edit visible.

The width matters too. On short functions a 5-gram separates a real edit from an unrelated function by about 2x; a 3-gram separates the same pair by about 2.5x, because a short function has too few 5-grams to survive a small change.

## The same idea for wire fields

A field's name is no more stable than a function's. `wre-signals` identifies a slot by its value across several runs rather than by its position:

```
wre align --before run-a1.json run-a2.json --after run-b1.json run-b2.json
```

A slot's signature is the tuple of values it held across every run. Slots whose signature is unique on both sides pair up; slots whose value appears more than once are reported as ambiguous rather than guessed at. More runs make more slots unique, which is why the command takes a list. Slots that differ between two runs of the *same* build are noise and are dropped before anything is concluded.

`dominant_shift` reports the most common displacement and how many slots agree with it, which is how a whole vector rotation shows up. It is deliberately not a "constant shift" test, because a rotation wraps and the wrapped slot never agrees.

## What is still text matching

`shape-text` clues are regexes, and `discovery` still finds the script tag in a document by pattern. Both are fine: the first runs over a name blind normalisation rather than the shipped bytes, and the second matches a URL, which is a deployment detail rather than a build artefact. The rule is that no role should depend on a single textual pattern to be found at all.
