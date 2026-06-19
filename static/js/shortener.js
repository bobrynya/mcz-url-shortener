// =============================================================================
// Auth — server-managed HttpOnly session cookie
//
// The session token lives in an HttpOnly cookie set by the server, so it is not
// readable from JavaScript. Same-origin requests send it automatically; login
// and logout go through dedicated server endpoints.
// =============================================================================
const Auth = {
  async logout() {
    await fetch('/dashboard/logout', { method: 'POST' }).catch(() => {});
    window.location.href = '/dashboard/login';
  },
  redirectToLogin() {
    window.location.href = '/dashboard/login';
  },
};

// =============================================================================
// Api — thin fetch wrapper
// Auth is carried by the HttpOnly session cookie (sent automatically same-origin).
// Returns: null (204 No Content), undefined (401, already redirected), {ok, status, data}
// =============================================================================
const Api = {
  async request(endpoint, options = {}) {
    const res = await fetch(endpoint, {
      ...options,
      headers: {
        'Content-Type': 'application/json',
        ...(options.headers || {}),
      },
    });
    if (res.status === 401) { Auth.redirectToLogin(); return undefined; }
    if (res.status === 204) return null;
    const data = toCamelCase(await res.json());
    return { ok: res.ok, status: res.status, data };
  },

  shorten(urls) {
    return Api.request('/api/shorten', { method: 'POST', body: JSON.stringify({ urls }) });
  },
  getLinks(params) {
    return Api.request(`/api/stats?${new URLSearchParams(clean(params))}`);
  },
  updateLink(code, patch) {
    return Api.request(`/api/links/${code}`, { method: 'PATCH', body: JSON.stringify(patch) });
  },
  deleteLink(code) {
    return Api.request(`/api/links/${code}`, { method: 'DELETE' });
  },
  getLinkStats(code, params) {
    return Api.request(`/api/stats/${code}?${new URLSearchParams(clean(params))}`);
  },
  getDomains() {
    return Api.request('/api/domains');
  },
  createDomain(data) {
    return Api.request('/api/domains', { method: 'POST', body: JSON.stringify(data) });
  },
  updateDomain(id, data) {
    return Api.request(`/api/domains/${id}`, { method: 'PATCH', body: JSON.stringify(data) });
  },
  deleteDomain(id) {
    return Api.request(`/api/domains/${id}`, { method: 'DELETE' });
  },
};

// =============================================================================
// Utilities
// =============================================================================
function toCamelCase(val) {
  if (Array.isArray(val)) return val.map(toCamelCase);
  if (val !== null && typeof val === 'object') {
    return Object.fromEntries(
      Object.entries(val).map(([k, v]) => [
        k.replace(/_([a-z])/g, (_, c) => c.toUpperCase()),
        toCamelCase(v),
      ])
    );
  }
  return val;
}

// Strip undefined/null/empty-string values before building URLSearchParams
function clean(params) {
  return Object.fromEntries(
    Object.entries(params || {}).filter(([, v]) => v !== undefined && v !== null && v !== '')
  );
}

function formatDate(str) {
  if (!str) return '—';
  const d = new Date(str), now = new Date();
  const diff = Math.floor((now - d) / 86400000);
  if (diff === 0) return 'Today';
  if (diff === 1) return 'Yesterday';
  if (diff < 7) return `${diff}d ago`;
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
}

function formatDateTime(str) {
  if (!str) return '—';
  return new Date(str).toLocaleString('en-US', {
    month: 'short', day: 'numeric', year: 'numeric',
    hour: '2-digit', minute: '2-digit',
  });
}

// =============================================================================
// loginPage() — Alpine data for /dashboard/login
// =============================================================================
function loginPage() {
  return {
    token: '',
    error: '',
    loading: false,

    async submit() {
      this.error = '';
      this.loading = true;
      try {
        // The server validates the token and sets an HttpOnly session cookie.
        const res = await fetch('/dashboard/login', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ token: this.token }),
        });
        if (res.ok) {
          window.location.href = '/dashboard';
        } else {
          this.error = res.status === 401 ? 'Invalid token' : 'Authentication failed';
        }
      } catch {
        this.error = 'Connection error';
      } finally {
        this.loading = false;
      }
    },
  };
}

// =============================================================================
// dashboardPage() — Alpine data for /dashboard
// =============================================================================
function dashboardPage() {
  return {
    domains: [],
    fields: [],
    recentLinks: [],
    results: [],
    loading: false,
    recentLoading: true,
    _nextId: 0,

    async init() {
      this.addField();
      await Promise.all([this.loadDomains(), this.loadRecent()]);
    },

    addField() {
      this.fields.push({
        id: this._nextId++,
        url: '',
        customCode: '',
        expiresAt: '',
        permanent: false,
        domainId: '',
        showAdvanced: false,
      });
    },

    removeField(id) {
      if (this.fields.length > 1) this.fields = this.fields.filter(f => f.id !== id);
    },

    async loadDomains() {
      const res = await Api.getDomains();
      if (res?.ok) this.domains = res.data.items.filter(d => d.isActive);
    },

    async loadRecent() {
      this.recentLoading = true;
      const res = await Api.getLinks({ page: 1, page_size: 20 });
      if (res?.ok) this.recentLinks = res.data.items;
      this.recentLoading = false;
    },

    async submit() {
      this.loading = true;
      this.results = [];

      const urls = this.fields.map(f => {
        const item = { url: f.url };
        if (f.domainId) {
          const d = this.domains.find(d => String(d.id) === String(f.domainId));
          if (d) item.domain = d.domain;
        }
        if (f.customCode) item.custom_code = f.customCode;
        if (f.expiresAt) item.expires_at = new Date(f.expiresAt).toISOString();
        if (f.permanent) item.permanent = true;
        return item;
      });

      const res = await Api.shorten(urls);
      this.loading = false;
      if (res?.ok) {
        this.results = res.data.items;
        this.fields = [];
        this.addField();
        await this.loadRecent();
      }
    },
  };
}

// =============================================================================
// linksPage() — Alpine data for /dashboard/links
// =============================================================================
function linksPage() {
  return {
    links: [],
    domains: [],
    domain: '',
    fromDate: '',
    toDate: '',
    pageSize: 25,
    page: 1,
    totalPages: 1,
    totalItems: 0,
    loading: false,
    editingCode: null,
    editForm: { url: '', expiresAt: '', permanent: false, restore: false },
    editError: '',
    deleteConfirm: null,

    get pages() {
      const delta = 2, arr = [];
      for (let i = Math.max(1, this.page - delta); i <= Math.min(this.totalPages, this.page + delta); i++) arr.push(i);
      return arr;
    },

    async init() {
      await Promise.all([this.loadDomains(), this.load()]);
    },

    async loadDomains() {
      const res = await Api.getDomains();
      if (res?.ok) this.domains = res.data.items;
    },

    async load() {
      this.loading = true;
      const res = await Api.getLinks({
        page: this.page,
        page_size: this.pageSize,
        domain: this.domain,
        from: this.fromDate ? new Date(this.fromDate).toISOString() : '',
        to: this.toDate ? new Date(this.toDate).toISOString() : '',
      });
      if (res?.ok) {
        this.links = res.data.items;
        this.totalPages = res.data.pagination.totalPages;
        this.totalItems = res.data.pagination.totalItems;
      }
      this.loading = false;
    },

    applyFilters() { this.page = 1; this.load(); },
    resetFilters() {
      this.domain = ''; this.fromDate = ''; this.toDate = '';
      this.pageSize = 25; this.page = 1; this.load();
    },
    goToPage(p) { this.page = p; this.load(); },

    statusBadge(link) {
      if (link.deletedAt) return { text: 'deleted', cls: 'bg-red-100 text-red-700' };
      if (link.expiresAt && new Date(link.expiresAt) < new Date()) return { text: 'expired', cls: 'bg-yellow-100 text-yellow-700' };
      return { text: 'active', cls: 'bg-green-100 text-green-700' };
    },

    startEdit(link) {
      this.editingCode = link.code;
      this.editError = '';
      this.editForm = { url: link.longUrl, expiresAt: '', permanent: false, restore: false };
    },
    cancelEdit() { this.editingCode = null; },

    async saveEdit(code) {
      this.editError = '';
      const patch = { url: this.editForm.url, permanent: this.editForm.permanent };
      if (this.editForm.expiresAt) patch.expires_at = new Date(this.editForm.expiresAt).toISOString();
      if (this.editForm.restore) patch.restore = true;
      const res = await Api.updateLink(code, patch);
      if (res?.ok) {
        this.editingCode = null;
        await this.load();
      } else {
        this.editError = res?.data?.error || 'Update failed';
      }
    },

    confirmDelete(code) { this.deleteConfirm = code; },
    cancelDelete() { this.deleteConfirm = null; },

    async doDelete(code) {
      const res = await Api.deleteLink(code);
      this.deleteConfirm = null;
      if (res === null) await this.load(); // 204 success
    },

    async copyLink(domain, code) {
      await navigator.clipboard.writeText(`https://${domain}/${code}`).catch(() => {});
    },
  };
}

// =============================================================================
// statsPage(code) — Alpine data for /dashboard/stats/{code}
// =============================================================================
function statsPage(code) {
  return {
    code,
    info: {},
    clicks: [],
    _chart: null,
    period: 'all',
    fromDate: '',
    toDate: '',
    showCustom: false,
    page: 1,
    totalPages: 1,
    loading: false,

    get pages() {
      const delta = 2, arr = [];
      for (let i = Math.max(1, this.page - delta); i <= Math.min(this.totalPages, this.page + delta); i++) arr.push(i);
      return arr;
    },

    shortUrl() {
      return this.info.domain ? `https://${this.info.domain}/${this.code}` : '';
    },

    async init() {
      await this.loadPeriod('all');
    },

    async setQuickFilter(p) {
      this.period = p;
      this.showCustom = p === 'custom';
      if (p !== 'custom') await this.loadPeriod(p);
    },

    async applyCustom() {
      this.period = 'custom';
      await this.loadPeriod('custom');
    },

    periodParams(p) {
      const now = new Date();
      if (p === 'today') {
        const s = new Date(now); s.setHours(0, 0, 0, 0);
        return { from: s.toISOString(), to: now.toISOString() };
      }
      if (p === 'week') {
        const s = new Date(now); s.setDate(now.getDate() - 7);
        return { from: s.toISOString(), to: now.toISOString() };
      }
      if (p === 'month') {
        const s = new Date(now); s.setMonth(now.getMonth() - 1);
        return { from: s.toISOString(), to: now.toISOString() };
      }
      if (p === 'custom') {
        return {
          from: this.fromDate ? new Date(this.fromDate).toISOString() : undefined,
          to: this.toDate ? new Date(this.toDate).toISOString() : undefined,
        };
      }
      return {};
    },

    async loadPeriod(p) {
      this.loading = true;
      this.page = 1;
      const res = await Api.getLinkStats(this.code, { page: 1, page_size: 50, ...this.periodParams(p) });
      if (res?.ok) {
        this.info = res.data;
        this.clicks = res.data.items || [];
        this.totalPages = res.data.pagination?.totalPages || 1;
      }
      this.loading = false;
      await this.$nextTick();
      this.renderChart();
    },

    async goToPage(p) {
      this.page = p;
      const res = await Api.getLinkStats(this.code, { page: p, page_size: 50, ...this.periodParams(this.period) });
      if (res?.ok) {
        this.clicks = res.data.items || [];
        this.totalPages = res.data.pagination?.totalPages || 1;
      }
    },

    async renderChart() {
      const el = document.getElementById('clicksChart');
      if (!el || typeof echarts === 'undefined') return;

      const res = await Api.getLinkStats(this.code, { page: 1, page_size: 1000, ...this.periodParams(this.period) });
      const all = res?.ok ? (res.data.items || []) : [];

      const counts = {};
      all.forEach(c => {
        const day = c.clickedAt.slice(0, 10);
        counts[day] = (counts[day] || 0) + 1;
      });

      const sorted = Object.keys(counts).sort();
      if (sorted.length === 0) return;

      const days = [], values = [];
      const start = new Date(sorted[0]), end = new Date(sorted[sorted.length - 1]);
      for (const d = new Date(start); d <= end; d.setDate(d.getDate() + 1)) {
        const k = d.toISOString().slice(0, 10);
        days.push(k);
        values.push(counts[k] || 0);
      }

      if (!this._chart) this._chart = echarts.init(el);
      this._chart.setOption({
        tooltip: { trigger: 'axis' },
        xAxis: { type: 'category', data: days, axisLabel: { rotate: 30, fontSize: 11 } },
        yAxis: { type: 'value', minInterval: 1 },
        series: [{
          data: values, type: 'line', smooth: true,
          areaStyle: { opacity: 0.25 },
          lineStyle: { color: '#3b82f6' },
          itemStyle: { color: '#3b82f6' },
        }],
        grid: { left: 40, right: 16, top: 16, bottom: 56 },
      });
    },
  };
}

// =============================================================================
// domainsPage() — Alpine data for /dashboard/domains
// =============================================================================
function domainsPage() {
  return {
    domains: [],
    loading: false,
    showAddForm: false,
    newDomain: { domain: '', isDefault: false, description: '' },
    addError: '',
    editingId: null,
    editForm: { isDefault: false, isActive: true, description: '' },
    editError: '',
    deleteConfirm: null,
    deleteError: '',

    async init() {
      await this.load();
    },

    async load() {
      this.loading = true;
      const res = await Api.getDomains();
      if (res?.ok) this.domains = res.data.items;
      this.loading = false;
    },

    async create() {
      this.addError = '';
      const data = { domain: this.newDomain.domain };
      if (this.newDomain.isDefault) data.is_default = true;
      if (this.newDomain.description) data.description = this.newDomain.description;
      const res = await Api.createDomain(data);
      if (res?.ok || res?.status === 201) {
        this.newDomain = { domain: '', isDefault: false, description: '' };
        this.showAddForm = false;
        await this.load();
      } else {
        this.addError = res?.data?.error || 'Failed to create domain';
      }
    },

    startEdit(d) {
      this.editingId = d.id;
      this.editError = '';
      this.editForm = { isDefault: d.isDefault, isActive: d.isActive, description: d.description || '' };
    },
    cancelEdit() { this.editingId = null; },

    async saveEdit(id) {
      this.editError = '';
      const data = { is_active: this.editForm.isActive, description: this.editForm.description || null };
      if (this.editForm.isDefault) data.is_default = true;
      const res = await Api.updateDomain(id, data);
      if (res?.ok) {
        this.editingId = null;
        await this.load();
      } else {
        this.editError = res?.data?.error || 'Update failed';
      }
    },

    confirmDelete(id) { this.deleteConfirm = id; this.deleteError = ''; },
    cancelDelete() { this.deleteConfirm = null; },

    async doDelete(id) {
      this.deleteError = '';
      const res = await Api.deleteDomain(id);
      if (res === null) {
        this.deleteConfirm = null;
        await this.load();
      } else if (res?.ok === false) {
        this.deleteError = res?.data?.error || 'Cannot delete domain';
        this.deleteConfirm = null;
      }
    },
  };
}
