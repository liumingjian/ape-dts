# Spec

## Summary

Define a unified end-to-end validation plan for the current GaussDB work so we
can verify the already-landed key features in a consistent order instead of
running isolated tests ad hoc.

This child starts with planning and evidence structure first, then subsequent
execution can follow the same matrix.

## Scope

- consolidate current high-value `dt-tests` and `scripts/e2e` entry points
- group them into layered suites:
  - quick gate
  - full functional gate
  - resilience gate
- cover the currently active GaussDB surfaces:
  - `PG <-> GaussDBPg`
  - `GaussDBPg -> PG` CDC resilience
  - `MySQL -> GaussDBMySQL`

## Acceptance

- child 5 taskmaster artifacts exist and are linked from the parent epic
- a readable docs-side e2e plan exists with exact commands and expected scope
- the matrix clearly distinguishes:
  - mandatory current regression set
  - optional long-running / environment-sensitive suites
  - evidence expectations and cleanup requirements
