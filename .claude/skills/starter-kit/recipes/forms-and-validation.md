# Recipe: forms, validation errors, precognition

**When:** any form that writes. Rails: `form_with` + `if @model.save … else render :new` (in
Inertia Rails: `redirect_to …, inertia: { errors: }`).

## The loop

1. The page renders Inertia's `<Form action={routes.update(id)}>`; every input has a `name`.
2. The handler parses with `Params<T>` (query + JSON or form body, unknown keys ignored).
3. Validate on the model. Success: `Redirect::to(path).notice("…")`. Failure:
   `Redirect::to(form_path).errors(errors)` or `Redirect::back(&headers, fallback).errors(errors)`.
4. The errors ride the flash cookie to the next GET and arrive as `errors`
   (`{field: string[]}`), which `<Form>` exposes: `errors.name?.map((message) => ({ message }))`
   into shadcn's `<FieldError>`.

Working examples: `src/controllers/settings/profiles.rs` + `frontend/pages/settings/profiles/show.tsx`
(hand-written), or any scaffolded resource (`src/controllers/<plural>.rs`, `form.tsx`).

## Model side

```rust
use crate::models::users::{Errors, SaveError};

let mut errors = Errors::new();
if name.trim().is_empty() { errors.add("name", "can't be blank"); }
if !errors.is_empty() { return Err(SaveError::Invalid(errors)); }
```

- Messages match Rails' defaults: "can't be blank", "is not a number", "has already been taken",
  "is too short (minimum is 12 characters)". Check the Rails kit when porting one.
- Forms submit strings. `src/models/cast.rs` casts them (`cast::string`, `number`,
  `optional_date`, `boolean`) and adds those messages; `#[serde(deserialize_with =
  "cast::form_value")]` also accepts JSON numbers/booleans from API clients.
- A field that must tell "missing" from `null`: `controllers::nullable` (see `settings/profiles.rs`).

## Precognition (validate as you type)

Server: before writing, answer a `Precognition: true` request with the errors and return:

```rust
if let Some(res) = precognitive(&headers, &params.errors()) {
    return Ok(res); // 422 {errors} or 204 + Precognition-Success; nothing written
}
```

`params.errors()` must not touch the database for writes, send mail, or spend a rate-limit
token. Endpoints that can't validate without side effects take `_: NoPrecognition` as their
**first** extractor, which answers 400. Scaffolded create/update already do the former and
destroy the latter.

Client: add `validate` calls to the fields:

```tsx
<Form action={projects.create()}>
  {({ errors, validate }) => (
    <Input name="name" onBlur={() => validate("name")} />
  )}
</Form>
```

## Error bags, several forms on one page

Give each `<Form>` an `errorBag="delete_user"`: the client sends it as `X-Inertia-Error-Bag`,
and the adapter nests that request's errors under the key, so two forms don't show each other's
errors. `Redirect::to(..).errors(e).error_bag("delete_user")` sets it from the server instead.
None of the kit's pages needs one yet; the behaviour is covered in `tests/inertia_b.rs`.

## Verify

```rust
let res = server.patch(path).json(&json!({ "name": "" })).await;
assert_redirect(&res, path);
let page = inertia_get(&server, &ctx, path).await;
assert_eq!(page["props"]["errors"], json!({ "name": ["can't be blank"] }));
```

Precognition: `tests/requests/precognition.rs` shows the pattern (asserts the database is
unchanged, 422 with errors, 204 when valid).
