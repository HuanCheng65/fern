import { sveltekit } from '@sveltejs/kit/vite';

// 站上用到的产品组件全部来自 fern-kit，按包名引——不再从 fern-ui 的源码树里
// 借文件，也不用在构建期给谁的样式表改作用域。
export default {
  plugins: [sveltekit()]
};
