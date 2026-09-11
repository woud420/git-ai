use super::{ExpectedLineExt, TestRepo};

#[test]
fn test_large_ai_readme_rewrite_with_no_data_bug() {
    // Regression test for bug where AI-authored lines show [no-data]
    // This replicates the exact scenario from commit a630f58cb9b1943cba895a38d00c4c4ed727e37c
    use std::fs;

    let repo = TestRepo::new();
    eprintln!("repo path: {:}", repo.path().to_str().unwrap());
    let file_path = repo.path().join("Readme.md");

    // First commit: Initial human content (exact content from the diff)
    fs::write(
        &file_path,
        "## A quick demo of Git AI Rewrites\n\ndasdas\n\nHUMAN",
    )
    .unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial README").unwrap();

    // Second commit: AI completely rewrites the README (exact content from the diff)
    fs::write(
        &file_path,
        "# Set Operations Library

A TypeScript library providing essential set operations for working with JavaScript `Set` objects. This library offers a collection of utility functions for performing common set operations like union, intersection, difference, and more.

## Features

This library provides the following set operations:

- **Union** - Combine all elements from two sets
- **Intersection** - Find elements common to both sets
- **Difference** - Find elements in the first set but not in the second
- **Symmetric Difference** - Find elements in either set but not in both
- **Superset Check** - Determine if one set contains all elements of another
- **Subset Check** - Determine if one set is contained within another

## Installation

Since this is a TypeScript project, you can use the functions directly by importing them:

```typescript
import { union, intersection, difference } from './set-ops';
// or
import { setUnion, setIntersect, setDiff } from './src/set-ops';
```

## Usage

### Basic Operations

```typescript
import { union, intersection, difference, symmetricDifference } from './set-ops';

// Create some sets
const setA = new Set([1, 2, 3, 4]);
const setB = new Set([3, 4, 5, 6]);

// Union: all elements from both sets
const unionResult = union(setA, setB);
// Result: Set { 1, 2, 3, 4, 5, 6 }

// Intersection: elements in both sets
const intersectionResult = intersection(setA, setB);
// Result: Set { 3, 4 }

// Difference: elements in setA but not in setB
const differenceResult = difference(setA, setB);
// Result: Set { 1, 2 }

// Symmetric Difference: elements in either set but not both
const symDiffResult = symmetricDifference(setA, setB);
// Result: Set { 1, 2, 5, 6 }
```

### Set Relationships

```typescript
import { isSuperset, isSubset } from './set-ops';

const setA = new Set([1, 2, 3, 4, 5]);
const setB = new Set([2, 3, 4]);

// Check if setA is a superset of setB
const isSuper = isSuperset(setA, setB);
// Result: true

// Check if setB is a subset of setA
const isSub = isSubset(setB, setA);
// Result: true
```

### Working with Different Types

All functions are generic and work with any type:

```typescript
// Strings
const fruitsA = new Set(['apple', 'banana', 'orange']);
const fruitsB = new Set(['banana', 'grape', 'apple']);
const allFruits = union(fruitsA, fruitsB);

// Objects (with proper comparison)
const usersA = new Set([{ id: 1 }, { id: 2 }]);
const usersB = new Set([{ id: 2 }, { id: 3 }]);
const allUsers = union(usersA, usersB);
```

## API Reference

### `union<T>(setA: Set<T>, setB: Set<T>): Set<T>`

Returns a new set containing all elements from both `setA` and `setB`.

### `intersection<T>(setA: Set<T>, setB: Set<T>): Set<T>`

Returns a new set containing only the elements that are present in both `setA` and `setB`.

### `difference<T>(setA: Set<T>, setB: Set<T>): Set<T>`

Returns a new set containing elements that are in `setA` but not in `setB`.

### `symmetricDifference<T>(setA: Set<T>, setB: Set<T>): Set<T>`

Returns a new set containing elements that are in either `setA` or `setB`, but not in both.

### `isSuperset<T>(set: Set<T>, subset: Set<T>): boolean`

Returns `true` if `set` contains all elements of `subset`, `false` otherwise.

### `isSubset<T>(set: Set<T>, superset: Set<T>): boolean`

Returns `true` if all elements of `set` are contained in `superset`, `false` otherwise.

## Notes

- All functions return new `Set` objects and do not modify the input sets
- Functions are generic and work with any type `T`
- Empty sets are handled correctly in all operations

## License

This project is open source and available for use.
"
    )
    .unwrap();

    // Mark the AI-authored content with mock_ai checkpoint
    repo.git_ai(&["checkpoint", "mock_ai", "Readme.md"])
        .unwrap();

    let commit = repo
        .stage_all_and_commit("AI rewrites README with set operations docs")
        .unwrap();

    // Verify that the commit has AI attestations
    assert_eq!(
        commit.authorship_log.attestations.len(),
        1,
        "Should have exactly one AI attestation"
    );

    // Verify line-by-line attribution for ALL lines
    let mut file = repo.filename("Readme.md");
    file.assert_lines_and_blame(crate::lines![
        "# Set Operations Library".ai(),
        "".human(),
        "A TypeScript library providing essential set operations for working with JavaScript `Set` objects. This library offers a collection of utility functions for performing common set operations like union, intersection, difference, and more.".ai(),
        "".human(),
        "## Features".ai(),
        "".ai(),
        "This library provides the following set operations:".ai(),
        "".ai(),
        "- **Union** - Combine all elements from two sets".ai(),
        "- **Intersection** - Find elements common to both sets".ai(),
        "- **Difference** - Find elements in the first set but not in the second".ai(),
        "- **Symmetric Difference** - Find elements in either set but not in both".ai(),
        "- **Superset Check** - Determine if one set contains all elements of another".ai(),
        "- **Subset Check** - Determine if one set is contained within another".ai(),
        "".ai(),
        "## Installation".ai(),
        "".ai(),
        "Since this is a TypeScript project, you can use the functions directly by importing them:".ai(),
        "".ai(),
        "```typescript".ai(),
        "import { union, intersection, difference } from './set-ops';".ai(),
        "// or".ai(),
        "import { setUnion, setIntersect, setDiff } from './src/set-ops';".ai(),
        "```".ai(),
        "".ai(),
        "## Usage".ai(),
        "".ai(),
        "### Basic Operations".ai(),
        "".ai(),
        "```typescript".ai(),
        "import { union, intersection, difference, symmetricDifference } from './set-ops';".ai(),
        "".ai(),
        "// Create some sets".ai(),
        "const setA = new Set([1, 2, 3, 4]);".ai(),
        "const setB = new Set([3, 4, 5, 6]);".ai(),
        "".ai(),
        "// Union: all elements from both sets".ai(),
        "const unionResult = union(setA, setB);".ai(),
        "// Result: Set { 1, 2, 3, 4, 5, 6 }".ai(),
        "".ai(),
        "// Intersection: elements in both sets".ai(),
        "const intersectionResult = intersection(setA, setB);".ai(),
        "// Result: Set { 3, 4 }".ai(),
        "".ai(),
        "// Difference: elements in setA but not in setB".ai(),
        "const differenceResult = difference(setA, setB);".ai(),
        "// Result: Set { 1, 2 }".ai(),
        "".ai(),
        "// Symmetric Difference: elements in either set but not both".ai(),
        "const symDiffResult = symmetricDifference(setA, setB);".ai(),
        "// Result: Set { 1, 2, 5, 6 }".ai(),
        "```".ai(),
        "".ai(),
        "### Set Relationships".ai(),
        "".ai(),
        "```typescript".ai(),
        "import { isSuperset, isSubset } from './set-ops';".ai(),
        "".ai(),
        "const setA = new Set([1, 2, 3, 4, 5]);".ai(),
        "const setB = new Set([2, 3, 4]);".ai(),
        "".ai(),
        "// Check if setA is a superset of setB".ai(),
        "const isSuper = isSuperset(setA, setB);".ai(),
        "// Result: true".ai(),
        "".ai(),
        "// Check if setB is a subset of setA".ai(),
        "const isSub = isSubset(setB, setA);".ai(),
        "// Result: true".ai(),
        "```".ai(),
        "".ai(),
        "### Working with Different Types".ai(),
        "".ai(),
        "All functions are generic and work with any type:".ai(),
        "".ai(),
        "```typescript".ai(),
        "// Strings".ai(),
        "const fruitsA = new Set(['apple', 'banana', 'orange']);".ai(),
        "const fruitsB = new Set(['banana', 'grape', 'apple']);".ai(),
        "const allFruits = union(fruitsA, fruitsB);".ai(),
        "".ai(),
        "// Objects (with proper comparison)".ai(),
        "const usersA = new Set([{ id: 1 }, { id: 2 }]);".ai(),
        "const usersB = new Set([{ id: 2 }, { id: 3 }]);".ai(),
        "const allUsers = union(usersA, usersB);".ai(),
        "```".ai(),
        "".ai(),
        "## API Reference".ai(),
        "".ai(),
        "### `union<T>(setA: Set<T>, setB: Set<T>): Set<T>`".ai(),
        "".ai(),
        "Returns a new set containing all elements from both `setA` and `setB`.".ai(),
        "".ai(),
        "### `intersection<T>(setA: Set<T>, setB: Set<T>): Set<T>`".ai(),
        "".ai(),
        "Returns a new set containing only the elements that are present in both `setA` and `setB`.".ai(),
        "".ai(),
        "### `difference<T>(setA: Set<T>, setB: Set<T>): Set<T>`".ai(),
        "".ai(),
        "Returns a new set containing elements that are in `setA` but not in `setB`.".ai(),
        "".ai(),
        "### `symmetricDifference<T>(setA: Set<T>, setB: Set<T>): Set<T>`".ai(),
        "".ai(),
        "Returns a new set containing elements that are in either `setA` or `setB`, but not in both.".ai(),
        "".ai(),
        "### `isSuperset<T>(set: Set<T>, subset: Set<T>): boolean`".ai(),
        "".ai(),
        "Returns `true` if `set` contains all elements of `subset`, `false` otherwise.".ai(),
        "".ai(),
        "### `isSubset<T>(set: Set<T>, superset: Set<T>): boolean`".ai(),
        "".ai(),
        "Returns `true` if all elements of `set` are contained in `superset`, `false` otherwise.".ai(),
        "".ai(),
        "## Notes".ai(),
        "".ai(),
        "- All functions return new `Set` objects and do not modify the input sets".ai(),
        "- Functions are generic and work with any type `T`".ai(),
        "- Empty sets are handled correctly in all operations".ai(),
        "".ai(),
        "## License".ai(),
        "".ai(),
        "This project is open source and available for use.".ai(),
    ]);
}
