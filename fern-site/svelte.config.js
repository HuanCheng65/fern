import adapter from '@sveltejs/adapter-static';

/** @type {import('@sveltejs/kit').Config} */
export default {
  kit: {
    adapter: adapter({ fallback: null, precompress: false }),
    prerender: { entries: ['*'] }
  }
};
