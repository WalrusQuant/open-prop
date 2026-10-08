<script lang="ts">
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";

  onMount(() => {
    void goto("/", { replaceState: true });
  });
</script>

<p class="waiting">Models are on the home page. <a href="/">Back</a></p>
