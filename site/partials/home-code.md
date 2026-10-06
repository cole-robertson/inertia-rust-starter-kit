<script setup>
import CodeCard from "../.vitepress/theme/CodeCard.vue"
import files from "../snippets/files.json"
</script>

<div class="code-cards">
<CodeCard :file="files['controller.rs']" icon="rust">

<<< @/snippets/controller.rs

</CodeCard>
<CodeCard :file="files['page.tsx']" icon="react">

<<< @/snippets/page.tsx

</CodeCard>
</div>

<p class="code-note"><code>ProjectProps</code> is a Rust struct; its TypeScript type is generated. Rename a field and the build tells you where.</p>
