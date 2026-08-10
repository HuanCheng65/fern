/**
 * 这台机器大概在哪儿。
 *
 * 只用系统时区和语言——不查 IP，也不问任何服务器。理由有两条：那两个信号
 * 手上真有，不需要为一次判断多发一个请求；而且它们不会因为一次网络异常就
 * 得出另一个结论。
 *
 * 判断结果只用来决定**默认值**和**要不要提供某个入口**，不用来限制已经存在
 * 的东西：猜错的代价必须是用户自己能改回来的。
 */

/** 系统区域看起来是中国大陆。 */
export function looksLikeChina(): boolean {
  try {
    const zone = Intl.DateTimeFormat().resolvedOptions().timeZone ?? ''
    if (/Shanghai|Chongqing|Harbin|Urumqi|Macau/i.test(zone)) return true
    return navigator.language.toLowerCase().startsWith('zh-cn')
  } catch {
    return false
  }
}

/**
 * 这里提不提供离线登录。
 *
 * 离线登录在不同地区的性质不一样：有的地方它是「正版买不到、支付走不通」时
 * 唯一能玩上的办法，有的地方它只是绕过购买。所以入口按地区给，而不是一律
 * 摆在第一屏——正版登录才是默认的那一条路。
 *
 * 这里只管**添加新的离线账户**这个入口。已经存在的离线账户照常能用、能切换、
 * 能启动：关掉一个入口是一回事，把用户已有的身份作废是另一回事。
 */
export function offlineLoginAllowed(): boolean {
  return looksLikeChina()
}
