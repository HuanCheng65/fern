/**
 * 账户名册。
 *
 * 界面这一侧只认识「谁是谁」：id、类型、名字、UUID、皮肤站。令牌从来不到
 * webview 里来，一个字节都没有（见 fern-core/src/accounts.rs）。所以这个
 * store 可以放心地被任何组件读——它拿不到任何能冒充你的东西。
 *
 * 名册的真身在磁盘上的 `accounts.json`，这里是它的一份镜像：每个写操作都
 * 走后端，回来之后整份重读。不做乐观更新——账户是身份，界面显示的必须是
 * 磁盘上真实存在的那一份，而不是一次可能失败的请求的样子。
 */

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { inTauri, instances } from './instances.svelte'
import { nav } from './nav.svelte'
import { commands, provides, type Action } from 'fern-kit/parts/palette'

export type AccountKind = 'offline' | 'microsoft' | 'authlib'

export interface Account {
  id: string
  kind: AccountKind
  playerName: string
  uuid: string
  /** 只有外置登录有。同一个名字在不同皮肤站是不同的人。 */
  apiRoot: string | null
  addedAt: number
}

/** 类型说成一句人话。`authlib` 这种词不该出现在界面上。 */
export const KIND_LABEL: Record<AccountKind, string> = {
  offline: '离线',
  microsoft: '微软账户',
  authlib: '外置登录',
}

/** 皮肤站地址里那个能认的部分。整条 URL 太长，域名足够区分是哪一家。 */
export function siteName(apiRoot: string | null): string {
  if (!apiRoot) return ''
  try {
    return new URL(apiRoot).hostname
  } catch {
    return apiRoot
  }
}

/**
 * 名字后面那一截：这个身份是从哪儿来的。
 *
 * 同一个名字可以合法地存在好几份——正版的 Steve、离线的 Steve、某个皮肤站上的
 * Steve。后端的去重键是 `(kind, uuid, api_root)`（见 `roster.rs::adopt_session`），
 * 所以判别式是**名字加出处**，光有名字分不开人。
 *
 * 外置写站点域名而不是「外置登录」四个字：同名号之间真正不同的就是那个站，
 * 而类型名对每一个外置号都一样，说了等于没说。
 */
export const originOf = (account: Account) =>
  account.apiRoot ? siteName(account.apiRoot) : KIND_LABEL[account.kind]

class AccountStore {
  list = $state<Account[]>([])
  activeId = $state('')
  loading = $state(false)
  error = $state('')
  /** 正版登录要用户去浏览器输的那八位码。只在等待的那段时间里有值。 */
  deviceCode = $state<{ userCode: string; verificationUri: string } | null>(null)
  /** 有一次登录正在进行。两次同时来只会互相打架。 */
  busy = $state(false)

  readonly active = $derived(
    this.list.find((account) => account.id === this.activeId) ?? this.list[0],
  )
  /** 启动、联机昵称、顶栏头像都读它。没有账户时是空串，调用方自己兜底。 */
  readonly playerName = $derived(this.active?.playerName ?? '')
  /**
   * 名册里出现不止一次的名字。
   *
   * 启动屏的题头默认只写名字，只有这个名字确实有第二个人在用时才把出处补上
   * 去——例外才发声。名单和切换器不看这个：那两处的职责就是分辨，永远写全。
   */
  readonly duplicated = $derived(
    new Set(
      this.list
        .map((account) => account.playerName)
        .filter((name, index, all) => all.indexOf(name) !== index),
    ),
  )

  async load() {
    if (!inTauri()) return
    this.loading = true
    try {
      const [list, active] = await Promise.all([
        invoke<Account[]>('list_accounts'),
        invoke<Account | null>('active_account'),
      ])
      this.list = list
      this.activeId = active?.id ?? ''
      this.error = ''
    } catch (error) {
      // 钥匙串打不开时这里会失败。那句话该说出来——账户列表空着而不解释，
      // 用户只会以为自己没登录过。
      this.error = String(error)
    } finally {
      this.loading = false
    }
  }

  async use(id: string) {
    if (!inTauri() || id === this.activeId) return
    try {
      await invoke('set_active_account', { id })
      this.activeId = id
    } catch (error) {
      this.error = String(error)
    }
  }

  async addOffline(playerName: string) {
    if (!inTauri()) return
    try {
      await invoke<Account>('add_offline_account', { playerName })
      this.error = ''
      await this.load()
    } catch (error) {
      this.error = String(error)
    }
  }

  async renameOffline(id: string, playerName: string) {
    if (!inTauri()) return
    try {
      await invoke<Account>('rename_offline_account', { id, playerName })
      this.error = ''
      await this.load()
    } catch (error) {
      this.error = String(error)
    }
  }

  async remove(id: string) {
    if (!inTauri()) return
    try {
      await invoke('remove_account', { id })
      this.error = ''
      await this.load()
    } catch (error) {
      this.error = String(error)
    }
  }

  /**
   * 微软正版。
   *
   * 八位码由后端在拿到之后推过来——它要显示的那一刻，登录还在等用户去浏览器
   * 里输。整个过程里密码和令牌都不经过 webview。
   */
  async loginMicrosoft() {
    if (!inTauri() || this.busy) return
    this.busy = true
    this.error = ''
    this.deviceCode = null
    const stop = await listen<{ userCode: string; verificationUri: string }>(
      'microsoft-device-code',
      ({ payload }) => (this.deviceCode = payload),
    )
    try {
      await invoke<Account>('microsoft_login')
      await this.load()
    } catch (error) {
      this.error = String(error)
    } finally {
      stop()
      this.deviceCode = null
      this.busy = false
    }
  }

  /** 外置登录。密码只在这一次调用里存在，换到令牌之后就没有用处了。 */
  async loginYggdrasil(apiRoot: string, username: string, password: string) {
    if (!inTauri() || this.busy) return
    this.busy = true
    this.error = ''
    try {
      await invoke<Account>('yggdrasil_login', { apiRoot, username, password })
      await this.load()
    } catch (error) {
      this.error = String(error)
    } finally {
      this.busy = false
    }
  }
}

export const accounts = new AccountStore()

/**
 * 这个实例按下启动会用谁。
 *
 * 实例只有两档：不钉住就跟着当前账户走，钉住了就用钉住的那一个。规则和后端
 * 的 `roster::for_instance` 是同一条，两边算出来的必须是同一个人——否则界面上
 * 写着一个名字，进游戏的是另一个。
 */
export const launchIdentity = (instanceId: string) => {
  const instance = instances.list.find((item) => item.id === instanceId)
  return accounts.list.find((item) => item.id === instance?.accountId) ?? accounts.active
}

/**
 * 在启动屏上换一个身份。
 *
 * 没钉住的实例换的是**当前账户**——「跟随」的意思就是它跟着这个值走，而题头
 * 上问候的正是这个人。钉住了的实例换的是那根钉子：一个已经声明「我单独用某个
 * 号」的实例，不该因为你在它的屏幕上换了个人就把别处也一起改掉。
 */
export async function useForLaunch(instanceId: string, accountId: string) {
  const instance = instances.list.find((item) => item.id === instanceId)
  if (instance?.accountId) await instances.setAccount(instanceId, accountId)
  else await accounts.use(accountId)
}

/** 切换器和名单里永远写全出处：那两处的职责就是把同名的人分开。 */
const asSubject = (item: Account) => ({
  type: 'account' as const,
  scoped: true,
  id: item.id,
  title: item.playerName,
  hint: originOf(item),
  seed: item.uuid,
  run: () => void switchTo(item.id),
})

/** 从哪儿选中的都一样：换的是「这次启动用谁」。 */
const switchTo = (id: string) => {
  const instance = instances.current
  return instance ? useForLaunch(instance.id, id) : accounts.use(id)
}

/**
 * 账户不平铺在顶层：它们不是这个面板的主角，一份有五个账户的名单会把实例和
 * 动作挤下去。所以走「动词需要宾语」那条路——搜「切换账户」，回车之后才列出
 * 名单，输入框左边挂上一枚 chip。
 */
provides(() => accounts.list.map(asSubject))

/** 档案是设置里的二级页，所以它的地址就是 `nav.focus` 的第三段。 */
const profileAt = (id: string) => `account/list/${id}`

/**
 * 换身份这个动作只有一份定义，两处使用：命令面板里搜得到，启动屏点题头也走
 * 它。启动屏那边把它当作下钻的动作传进面板，所以「点名字换一个」和「⌘K 换一
 * 个」不会各自长出一套规则。
 */
export const switchAction = (): Action => ({
  id: 'account.switch',
  title: '切换账户',
  hint: accounts.playerName,
  accepts: 'account',
  // 故意不给默认宾语：这个动词的全部意义就是换一个，一步到位没有意义。
  run: (subject) => {
    if (subject) void switchTo(subject.id)
  },
})

commands(() => [
  switchAction(),
  {
    id: 'account.profile',
    title: '账户档案',
    hint: accounts.playerName,
    accepts: 'account',
    // 默认宾语是当前账户：多数时候「看看我的档案」说的就是它。
    subject: () => {
      const item = accounts.active
      if (!item) return undefined
      return {
        type: 'account' as const,
        id: item.id,
        title: item.playerName,
        hint: KIND_LABEL[item.kind],
        seed: item.uuid,
        run: () => nav.show('settings', profileAt(item.id)),
      }
    },
    run: (subject) => {
      if (subject) nav.show('settings', profileAt(subject.id))
    },
  },
  {
    id: 'account.add',
    title: '添加账户',
    accepts: 'none',
    // 名单里没有你要的那个人时，出口就在名单的最后一行。
    creates: 'account',
    run: () => nav.show('settings', profileAt('new')),
  },
])
