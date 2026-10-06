<script setup>
import CodeCard from "../.vitepress/theme/CodeCard.vue"
import files from "../snippets/files.json"
</script>

## A controller and the page it renders

<p class="lede">The controller loads the data and hands it to a React page as props. There's no JSON API in between, and nothing to keep in sync.</p>

<div class="code-cards">
<CodeCard :file="files['controller.rs']" icon="rust">

<<< @/snippets/controller.rs

</CodeCard>
<CodeCard :file="files['page.tsx']" icon="react">

<<< @/snippets/page.tsx

</CodeCard>
</div>

`cargo loco generate scaffold projects name:string!` writes both, plus the model, migration, routes and tests.
