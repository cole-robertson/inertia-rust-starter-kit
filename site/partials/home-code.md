## A controller and the page it renders

<p class="lede">The controller loads the data and hands it to a React page as props. There's no JSON API in between, and nothing to keep in sync.</p>

::: code-group

<<< @/snippets/controller.rs [Rust controller]

<<< @/snippets/page.tsx [React page]

:::

`cargo loco generate scaffold projects name:string!` writes both, plus the model, migration, routes and tests.
