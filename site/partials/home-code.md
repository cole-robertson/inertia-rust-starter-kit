<script setup>
import CodeCard from "../.vitepress/theme/CodeCard.vue"
import files from "../snippets/files.json"
</script>

## A controller and the page it renders

<p class="lede">The controller loads the data and hands it to a React page as props. There's no JSON API in between, and the page's types come from the Rust struct.</p>

<div class="code-cards">
<CodeCard :file="files['controller.rs']" icon="rust">

<<< @/snippets/controller.rs

</CodeCard>
<CodeCard :file="files['page.tsx']" icon="react">

<<< @/snippets/page.tsx

</CodeCard>
</div>

`ProjectProps` is generated from the Rust struct: rename a field and the TypeScript build fails. `cargo loco generate scaffold projects name:string!` writes both, plus the model, migration, routes and tests.
