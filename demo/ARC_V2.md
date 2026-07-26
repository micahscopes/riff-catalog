# Riffcat demo arc, draft two

> Planning rationale only. Audience-facing copy has one source of truth:
> `demo/app/app.js`, in the `CH` slide records. Do not copy-edit this document
> independently of the running deck.

## The narrative invariant

Software work is continually translated: source becomes syntax trees, IR,
bytecode, traces, models, specifications, and proofs. Each representation makes
some facts visible and hides others. Today, the facts we learn in one place are
usually attached to that representation's name, file, or build and are lost when
the representation changes.

Riffcat gives the part that matters to a particular question a stable address.
An artifact can therefore have several useful addresses: exact content, shape,
type interface, dependency surface, provenance-bearing form, or another declared
facet. Tools can meet at the narrowest address that preserves the fact they want
to share. The address does not prove semantic equivalence; it makes the scope of
the claim explicit and gives proofs, attestations, and observations somewhere
precise to attach.

The project is useful when it helps knowledge survive translation without
pretending that every translation preserves the same thing.

## The four questions at deck scale

1. **What problem are we solving?** Representations change faster than the
   knowledge attached to them can travel. Names and whole-artifact hashes are
   either too contextual or too brittle to reconnect that knowledge.
2. **Who is affected?** Compiler engineers, verification researchers, security
   reviewers, debugger and tracing authors, dependency and build-system authors,
   agent-tooling builders, and ultimately anyone relying on the software they
   produce.
3. **Why are we poised to solve it?** We have a working faceted fingerprinting
   engine, real source and compiler-artifact experiments, provenance hooks, a
   cross-language schema, and formalizations of the addressing rules. The idea
   also has strong precedent: URIs separate identification from access; Lurk
   makes values address-shaped inside proving computation; Ix addresses Lean
   declarations modulo a carefully chosen invariant.
4. **What do we need?** Co-design. We need compiler teams to expose provenance,
   verification teams to define useful proof obligations and bridge theorems,
   application teams to identify the facts worth transporting, and shared
   collections of real examples to learn when each address is useful or unsafe.

## Slide arc

### 1. Every tool meets a different program

**Job:** Establish the world of representations before introducing similarity.

Source, AST, IR, optimized IR, bytecode, trace, model, and proof are not merely
formats. Each is a perspective built to answer a different question. The loss:
knowledge attached to one perspective rarely survives the next translation.

**Four-question pressure:** problem first; everyone who works across tool
boundaries should recognize themselves.

### 2. Which differences matter to your question?

**Job:** Introduce the choice riffcat makes explicit.

Use the five-function facet dial. A refactor tool may care about names; a clone
finder may not; a type-directed dependency resolver may care about the interface
and ignore the body. There is no universally correct notion of sameness.

**Four-question pressure:** the impacted person supplies the question; riffcat
does not impose the invariant.

### 3. A riff survives a change of key

**Job:** Let a diverse audience feel the idea before asking them to parse it.

Use the music dial. Notes, intervals, and rhythm preserve different aspects of
one musical idea. “Similarity is a spectrum” belongs here as supporting copy,
not as the thesis headline.

### 4. Give what survives an address

**Job:** Deliver the mechanism.

Canonicalize only the declared variation, then content-address the result. The
address is a stable handle for that chosen perspective. One artifact may have
several such handles.

### 5. Known code hides in plain sight

**Job:** Show an immediate, legible payoff on real code.

Recognize library functions across verified contracts even when contextual
presentation changes. The result is not “these programs mean the same thing”; it
is “this known structure occurs here, and here is exactly what matched.”

### 6. A bug can outlive its spelling

**Job:** Turn recognition into stakes.

Show a known vulnerable shape and edited forks. Exact source and name search
miss variations; a declared structural address finds candidates and localizes
the matching subtree. Keep “candidate” visibly distinct from “verdict.”

### 7. Compilation should not erase the trail

**Job:** Connect source, compiler artifacts, and tracing.

The same addressed structure can carry origin edges through lowering. Where an
address survives, provenance can reconnect source, IR, optimized output, and
debug traces. Where it changes, the divergence becomes local and inspectable.

### 8. One artifact needs more than one address

**Job:** Prevent the audience from reducing riffcat to a fuzzy hash.

Contrast exact-build, shape, type-interface, and provenance-sensitive addresses.
Each answers a different question. A strict checksum is the bottom rung, not a
rival system.

### 9. Structure narrows the proof

**Job:** Join syntactic and semantic equivalence without collapsing them.

Syntactic equalities are cheap, decidable relations. Semantic equivalence is a
claim under a model. A verifier can prove a bridge from a chosen syntactic facet
to the observation it preserves, then reuse that bridge wherever the address
recurs: prove the bridge once; reuse it at every shared address.

### 10. A match tells us what to prove

**Job:** Make the boundary between a match and a proof unmistakable.

Riffcat localizes a candidate and emits a scoped proof obligation. A named
verifier proves equivalence or supplies a counterexample. Matching is useful
because it makes the expensive question smaller, not because it answers it.

### 11. A fact travels only as far as its anchor

**Job:** State the heart of the project interactively.

Attach an observation, attestation, proof, or warning to an address. It travels
to every occurrence of that address and no farther. Loosen the facet past a
dimension the fact depends on and the attachment becomes unsound; the demo must
show that failure, not conceal it.

### 12. Shared references have changed systems before

**Job:** Establish precedent without selling by analogy.

- URIs let heterogeneous systems share a way to point without sharing a
  representation or access mechanism; URI comparison itself has several rungs.
- Lurk makes compound values content-addressable inside a proving computation,
  joining evaluation, storage, commitments, and proofs.
- Ix canonicalizes Lean declarations modulo chosen cosmetic variation so a
  typechecking proof can be reused wherever the same address appears.

The synthesis: URIs provide shared pointing; Lurk joins addressed computation
with proof; Ix demonstrates deliberately scoped canonical identity. Riffcat
makes several such scopes available across representations.

### 13. Pin the dependency you actually mean

**Job:** Open from the mechanism into the larger vision.

Show four concise possibilities, not an exhaustive product list:

- resolve a dependency to a type interface rather than a package name;
- keep compiler and FV tooling pointed at the same object across models;
- reconnect source maps, traces, and debug facts after transformations;
- attach attestations and proofs to precisely the structure they establish.

Agents and the HHHS substrate belong as additional consumers of the primitive,
not as a new chapter or a pivot in the story.

### 14. The same handle can join the tools

**Job:** Show why this is a shared substrate rather than a standalone search UI.

The compiler produces representations and provenance. Riffcat supplies scoped
addresses and localization. Verifiers and analysis tools attach or discharge
claims. A content-addressed database transports the resulting graph. No one
component closes the loop alone.

### 15. The boundary is part of the result

**Job:** Earn trust before the invitation.

Name locality limits, cycles, context-dependent instantiation, missing labelled
evaluation data, and the distinction between demonstrations, plans, and checked
proofs. The honest boundary demonstrates that facets are claims with scopes, not
magic similarity settings.

### 16. What could your work reuse?

**Job:** Turn “what do we need?” into an invitation to join.

Offer the working library and schema for co-design. Ask compiler teams which
origins they can emit, verification teams what makes a candidate worth proving,
tool authors what facts they need to survive transformations, and project owners
which set of real examples would be credible. End on participation, not adoption.

## Material moved out of the main arc

The detailed Forte catalog, interval vectors, Merkle fold walkthrough,
Sourcify bytecode metadata anatomy, exact sample ledger, engine lockstep plan,
and individual FV case studies remain valuable appendix chapters. They answer
expert follow-ups but interrupt the main causal line when presented by default.
