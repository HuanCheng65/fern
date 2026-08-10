<script>
  import Hero from '$lib/sections/Hero.svelte';
  import Ready from '$lib/sections/Ready.svelte';
  import Design from '$lib/sections/Design.svelte';
  import Direct from '$lib/sections/Direct.svelte';
  import Supply from '$lib/sections/Supply.svelte';
  import Runtime from '$lib/sections/Runtime.svelte';
  import Diagnose from '$lib/sections/Diagnose.svelte';
  import Backup from '$lib/sections/Backup.svelte';
  import Pearl from '$lib/sections/Pearl.svelte';
  import Privacy from '$lib/sections/Privacy.svelte';
  import Platforms from '$lib/sections/Platforms.svelte';
  import Get from '$lib/sections/Get.svelte';
</script>

<svelte:head>
  <title>Fern — 一个为 Minecraft 打造的现代启动器</title>
</svelte:head>

<main>
  <Hero />
  <Ready />
  <Design />
  <Direct />
  <Supply />
  <Runtime />
  <Diagnose />
  <Backup />
  <Pearl />
  <Privacy />
  <Platforms />
  <Get />
</main>
