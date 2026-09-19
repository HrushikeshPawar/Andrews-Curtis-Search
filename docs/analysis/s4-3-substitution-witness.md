# S4.3 substitution witness (code-level)

This note pins the ACSolverX / GS-Sub substitution as a **witnessed**
macro over official primitive AC moves. Trusted search state remains the
exact ordered freely reduced presentation. S4.2b (`supermove.rs`) is left
frozen: it rotates both relators by a short conjugator and keeps the
source rotated. This experiment restores the source and cyclic-reduces
the written relator.

ACSolverX treats Booth lex-min rotation of the written relator as free
and recognises any two length-1 relators as trivial. Both are excluded
here. Destination states are not canonicalized. The exact SAIR target
remains `[[1],[2]]`.

## One proposal

Parameters:

| field | meaning |
|---|---|
| `target` | written relator, `0` or `1` |
| `invert_other` | use `other^{-1}` as the multiplied word |
| `rot_target` | left-rotation of the target before multiplying |
| `rot_other` | left-rotation of the (possibly inverted) other relator |
| `left_multiply` | `false`: `T <- T * O`; `true`: `T <- O * T` |

Word-level destination (other relator unchanged):

```
T' = cyclic_reduce(  rot(T, kt) * rot(maybe_inv(O), ko) )   // right
T' = cyclic_reduce(  rot(maybe_inv(O), ko) * rot(T, kt) )   // left
```

A proposal is kept only when the product construction actually cancels
letters (`|rot T| + |rot O| - |T'| > 0`) and `T'` differs from `T`.

## Rotations as conjugations

Left-rotate `w = prefix * rest` by `|prefix|` is conjugation by
`prefix^{-1}`:

```
prefix^{-1} * (prefix * rest) * prefix = rest * prefix
```

Official conjugations are one letter. Applying `conj(target, -a)` for
each letter `a` of the prefix, in order, realises that conjugation.
Un-rotation is the inverse move sequence (reverse order, each conjugator
negated).

## Optional inversion

`invert_other` is official invert of the other relator (`move 0` or
`1`), used only as a setup:

1. invert other
2. rotate other
3. multiply (no `invert_other` flag; the stored other is already inverted)
4. un-rotate other
5. invert other again

When `rot_other == 0`, the invert/uninvert pair is replaced by a single
official multiply with `invert_other = true`, which never touches the
stored other relator.

## Multiplication

Right multiply is official `r_target <- r_target * r_other` (ids 2..=5).

Left multiply `r_target <- r_other * r_target` is the 3-move gadget:

1. invert target
2. `target <- target * other^{-1}`
3. invert target

Net: `target' = other * target`, other unchanged.

## Cyclic reduction

SAIR free reduction only cancels **adjacent** inverse pairs. Wrap-around
pairs (`T'[0] == -T'[last]`) are not free. Each such pair is removed by
conjugating the target by its last letter, which is official and
length-reducing. Repeat until the written relator has no wrap-around
inverse pair.

## What ACSolverX does that we must not copy

- **Booth lex-min rotation** of the written relator after the product:
  free cyclic canonicalization. Excluded. If a lex-min rotation is
  wanted, it would have to be an explicit conjugation witness; this
  solver does not emit it.
- **Triviality = two nonzero letters.** Excluded. Only exact `[[1],[2]]`
  (or a bank/suffix that replays to it) is a solution.
- **Always concatenate `r0 * r1` and always invert relator 1** for the
  computational copy. We generalise to `target`/`other` as specified
  above; both orientations are witnessed.
- **Length cap / silent noop** when the product exceeds a padded
  `max_length`. Not a SAIR rule; we do not noop. The search still
  respects `max_total_relator_length`.

## Hand-checkable examples

See `substitution.rs` tests. Summary:

1. `[[1,2],[2]]`, right, invert other, `kt=ko=0`: move `3`, destination
   `[[1],[2]]`.
2. `[[1],[2,1]]`, right, invert other, `kt=ko=0`: move `5`, destination
   `[[1],[2]]`.
3. `[[2,2,1],[-2,-1]]`, right, `kt=1`, `ko=1`: junction-free product
   `[2,1,2,-1,-2]` cyclic-reduces to `[2]`; other relator restored to
   `[-2,-1]`. Destination `[[2],[-2,-1]]`.
4. `[[2,1,2],[-2]]`, right, `kt=ko=0`: official multiply yields `[2,1]`,
   which is **not** Booth-rotated to `[1,2]`.
