/**
 * URL Shortener - JavaScript Library (UPDATED)
 * Библиотека для работы с сокращателем ссылок
 */

/**
 * ============================================
 * AUTH MODULE
 * ============================================
 */

let Auth = {
    /**
     * Получение токена из cookie
     */
    getToken() {
        const cookies = document.cookie.split(';');
        for (let cookie of cookies) {
            const [name, value] = cookie.trim().split('=');
            if (name === 'auth_token') {
                return value;
            }
        }
        return null;
    },

    /**
     * Выход из системы
     */
    logout() {
        // Удаляем cookie
        document.cookie = 'auth_token=; path=/; max-age=0';

        // Редирект на login
        window.location.href = '/dashboard/login';
    }
};


// ============================================
// UTILITY FUNCTIONS (Утилитарные функции)
// ============================================

const Utils = {
    /**
     * Форматирование даты в читаемый вид
     * @param {string} dateString - ISO строка даты (RFC3339)
     * @returns {string} Отформатированная дата
     */
    formatDate(dateString) {
        const date = new Date(dateString);
        const now = new Date();
        const diff = now - date;
        const days = Math.floor(diff / (1000 * 60 * 60 * 24));

        if (days === 0) return 'Сегодня';
        if (days === 1) return 'Вчера';
        if (days < 7) return `${days} дн. назад`;

        return date.toLocaleDateString('ru-RU', {
            year: 'numeric',
            month: 'short',
            day: 'numeric',
            hour: '2-digit',
            minute: '2-digit'
        });
    },

    /**
     * Форматирование даты и времени полностью
     * @param {string} dateString - ISO строка даты
     * @returns {string} Полная дата и время
     */
    formatDateTime(dateString) {
        const date = new Date(dateString);
        return date.toLocaleDateString('ru-RU', {
            year: 'numeric',
            month: 'long',
            day: 'numeric',
            hour: '2-digit',
            minute: '2-digit',
            second: '2-digit'
        });
    },

    /**
     * Копирование текста в буфер обмена
     * @param {string} text - Текст для копирования
     * @param {HTMLElement} button - Кнопка, которая инициировала копирование
     */
    async copyToClipboard(text, button) {
        try {
            await navigator.clipboard.writeText(text);
            const originalText = button.textContent;
            button.textContent = '✔';
            button.style.background = '#10b981';

            setTimeout(() => {
                button.textContent = originalText;
                button.style.background = '';
            }, 2000);
        } catch (err) {
            console.error('Ошибка копирования:', err);
            alert('Не удалось скопировать в буфер обмена');
        }
    },

    /**
     * Отображение ошибки
     * @param {string} message - Сообщение об ошибке
     * @param {HTMLElement} container - Контейнер для ошибки
     */
    showError(message, container) {
        container.innerHTML = `<div class="error">${message}</div>`;
    },

    /**
     * Отображение состояния загрузки
     * @param {HTMLElement} container - Контейнер для индикатора
     */
    showLoading(container) {
        container.innerHTML = '<div class="loading">Загрузка...</div>';
    },

    /**
     * Отображение пустого состояния
     * @param {string} message - Сообщение
     * @param {HTMLElement} container - Контейнер
     */
    showEmpty(message, container) {
        container.innerHTML = `<div class="empty-state"><p>${message}</p></div>`;
    }
};


// ============================================
// API MODULE (Работа с API)
// ============================================

const API = {
    /**
     * Базовый метод для API запросов
     * @param {string} endpoint - Эндпоинт API
     * @param {object} options - Опции fetch
     * @returns {Promise} Промис с данными
     */
    async request(endpoint, options = {}) {
        const token = Auth.getToken();

        const response = await fetch(endpoint, {
            headers: {
                'Content-Type': 'application/json',
                'Authorization': token ? `Bearer ${token}` : '',
                ...options.headers
            },
            ...options
        });

        // Если 401 - токен невалиден
        if (response.status === 401) {
            alert('Сессия истекла. Необходимо войти заново.');
            Auth.logout();
            return null;
        }

        // Если другая ошибка - бросаем исключение
        if (!response.ok) {
            const error = new Error(`HTTP error! status: ${response.status}`);
            console.error('API Error:', error);
            throw error;
        }

        return await response.json();
    },

    /**
     * Получение списка доменов
     * @returns {Promise} Промис со списком доменов
     */
    async getDomains() {
        return this.request('/api/domains/');
    },

    /**
     * Получение списка ссылок с фильтрами
     * @param {object} params - Параметры (page, page_size, from, to, domain)
     * @returns {Promise} Промис со списком ссылок
     */
    async getLinks(params = {}) {
        const queryParams = new URLSearchParams();

        if (params.page) queryParams.append('page', params.page);
        if (params.page_size) queryParams.append('page_size', params.page_size);
        if (params.from) queryParams.append('from', params.from);
        if (params.to) queryParams.append('to', params.to);
        if (params.domain) queryParams.append('domain', params.domain);

        const query = queryParams.toString();
        return this.request(`/api/stats${query ? '?' + query : ''}`);
    },

    /**
     * Создание коротких ссылок (множественное)
     * @param {Array} urls - Массив объектов {url, custom_code?, domain?}
     * @returns {Promise} Промис с результатами
     */
    async createLinks(urls) {
        return this.request('/api/shorten', {
            method: 'POST',
            body: JSON.stringify({ urls })
        });
    },

    /**
     * Получение статистики по конкретной ссылке
     * @param {string} code - Короткий код ссылки
     * @param {object} params - Параметры (page, page_size, from, to)
     * @returns {Promise} Промис со статистикой
     */
    async getLinkStats(code, params = {}) {
        const queryParams = new URLSearchParams();

        if (params.page) queryParams.append('page', params.page);
        if (params.page_size) queryParams.append('page_size', params.page_size);
        if (params.from) queryParams.append('from', params.from);
        if (params.to) queryParams.append('to', params.to);

        const query = queryParams.toString();
        return this.request(`/api/stats/${code}${query ? '?' + query : ''}`);
    }
};


// ============================================
// DASHBOARD MODULE (Модуль главной страницы)
// ============================================

const Dashboard = {
    domains: [],
    linkFields: [],

    /**
     * Загрузка списка доменов
     */
    async loadDomains() {
        try {
            const data = await API.getDomains();
            this.domains = data.items || [];
        } catch (error) {
            console.error('Ошибка загрузки доменов:', error);
            this.domains = [];
        }
    },

    /**
     * Загрузка последних созданных ссылок
     */
    async loadRecentLinks() {
        const container = document.getElementById('recentLinks');
        Utils.showLoading(container);

        try {
            const data = await API.getLinks({ page: 1, page_size: 20 });

            if (!data.items || data.items.length === 0) {
                Utils.showEmpty('Нет созданных ссылок', container);
                return;
            }

            container.innerHTML = `
                <table>
                    <thead>
                        <tr>
                            <th>Короткая ссылка</th>
                            <th>Оригинальная ссылка</th>
                            <th>Домен</th>
                            <th>Переходы</th>
                            <th>Создана</th>
                            <th>Действия</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${data.items.map(link => `
                            <tr>
                                <td>
                                    <a href="https://${link.domain}/${link.code}" target="_blank">
                                        ${link.code}
                                    </a>
                                </td>
                                <td style="max-width: 300px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;" 
                                    title="${link.long_url}">
                                    ${link.long_url}
                                </td>
                                <td><code>${link.domain}</code></td>
                                <td>${link.total || 0}</td>
                                <td>${Utils.formatDate(link.created_at)}</td>
                                <td>
                                    <div class="actions">
                                        <a href="/dashboard/stats/${link.code}" class="btn btn-sm">Статистика</a>
                                        <button onclick="Utils.copyToClipboard('https://${link.domain}/${link.code}', this)" 
                                                class="btn btn-sm btn-secondary">📋</button>
                                    </div>
                                </td>
                            </tr>
                        `).join('')}
                    </tbody>
                </table>
            `;
        } catch (error) {
            Utils.showError('Ошибка загрузки ссылок: ' + error.message, container);
        }
    },

    /**
     * Добавление нового поля для ссылки
     */
    addLinkField() {
        const container = document.getElementById('linkFieldsContainer');
        const fieldId = Date.now();
        this.linkFields.push(fieldId);

        const domainOptions = this.domains
            .filter(d => d.is_active)
            .map(d => `<option value="${d.domain}" ${d.is_default ? 'selected' : ''}>${d.domain}</option>`)
            .join('');

        const fieldHTML = `
            <div class="link-field" id="field-${fieldId}">
                <div class="link-field-header">
                    <h3>Ссылка #${this.linkFields.length}</h3>
                    ${this.linkFields.length > 1 ? `<button type="button" onclick="Dashboard.removeLinkField(${fieldId})" class="btn btn-sm btn-danger">X</button>` : ''}
                </div>
                <div class="link-field-content">
                    <div class="form-group">
                        <label>Оригинальная ссылка *</label>
                        <input type="url" 
                               class="link-url" 
                               data-field-id="${fieldId}"
                               required 
                               placeholder="https://example.com/very/long/url">
                    </div>
                    <div class="form-row">
                        <div class="form-group">
                            <label>Домен</label>
                            <select class="link-domain" data-field-id="${fieldId}">
                                ${domainOptions}
                            </select>
                        </div>
                        <div class="form-group">
                            <label>Пользовательский код</label>
                            <input type="text" 
                                   class="link-custom-code" 
                                   data-field-id="${fieldId}"
                                   pattern="[a-zA-Z0-9_-]+" 
                                   placeholder="my-custom-link">
                            <small>Только буквы, цифры, дефис и подчеркивание</small>
                        </div>
                    </div>
                </div>
            </div>
        `;

        container.insertAdjacentHTML('beforeend', fieldHTML);
    },

    /**
     * Удаление поля ссылки
     */
    removeLinkField(fieldId) {
        const field = document.getElementById(`field-${fieldId}`);
        if (field) {
            field.remove();
            this.linkFields = this.linkFields.filter(id => id !== fieldId);
            this.updateFieldNumbers();
        }
    },

    /**
     * Обновление номеров полей
     */
    updateFieldNumbers() {
        const fields = document.querySelectorAll('.link-field');
        fields.forEach((field, index) => {
            const header = field.querySelector('.link-field-header h3');
            if (header) {
                header.textContent = `Ссылка #${index + 1}`;
            }
        });
    },

    /**
     * Сбор данных из формы
     */
    collectFormData() {
        const urls = [];
        const urlInputs = document.querySelectorAll('.link-url');

        urlInputs.forEach(input => {
            const fieldId = input.dataset.fieldId;
            const url = input.value.trim();

            if (url) {
                const domainSelect = document.querySelector(`.link-domain[data-field-id="${fieldId}"]`);
                const customCodeInput = document.querySelector(`.link-custom-code[data-field-id="${fieldId}"]`);

                const linkData = { url };

                const domain = domainSelect?.value;
                const defaultDomain = this.domains.find(d => d.is_default)?.domain;
                if (domain && domain !== defaultDomain) {
                    linkData.domain = domain;
                }

                const customCode = customCodeInput?.value.trim();
                if (customCode) {
                    linkData.custom_code = customCode;
                }

                urls.push(linkData);
            }
        });

        return urls;
    },

    /**
     * Отображение результатов создания
     */
    displayResults(response) {
        const container = document.getElementById('createResult');
        const { summary, items } = response;

        let html = `
            <div class="result">
                <h2>${summary.successful > 0 ? '✓' : '✗'} Создано ${summary.successful} из ${summary.total} ссылок</h2>
                <div class="results-list">
        `;

        items.forEach(item => {
            if (item.error) {
                html += `
                    <div class="result-item result-error">
                        <div class="result-url-display">
                            <span class="result-icon">✗</span>
                            <span class="result-long-url">${item.long_url}</span>
                        </div>
                        <div class="result-message error">
                            ${item.error.message}
                            ${item.error.details ? `<br><small>${JSON.stringify(item.error.details)}</small>` : ''}
                        </div>
                    </div>
                `;
            } else {
                html += `
                    <div class="result-item result-success">
                        <div class="result-url-display">
                            <span class="result-icon">✓</span>
                            <span class="result-long-url">${item.long_url}</span>
                        </div>
                        <div class="result-short">
                            <input type="text" value="${item.short_url}" readonly>
                            <button onclick="Utils.copyToClipboard('${item.short_url}', this)" class="btn btn-sm">
                                📋
                            </button>
                            <a href="/dashboard/stats/${item.code}" class="btn btn-sm btn-secondary">Статистика</a>
                        </div>
                    </div>
                `;
            }
        });

        html += `
                </div>
            </div>
        `;

        container.innerHTML = html;
    },

    /**
     * Обработка создания ссылок
     */
    async handleCreateLinks(event) {
        event.preventDefault();

        const submitBtn = event.target.querySelector('button[type="submit"]');
        const originalBtn = submitBtn.textContent;

        submitBtn.disabled = true;
        submitBtn.textContent = 'Создание...';

        const urls = this.collectFormData();

        if (urls.length === 0) {
            alert('Добавьте хотя бы одну ссылку');
            submitBtn.disabled = false;
            submitBtn.textContent = originalBtn;
            return;
        }

        try {
            const result = await API.createLinks(urls);

            // Показываем результаты
            this.displayResults(result);

            // Если есть успешные, обновляем список
            if (result.summary.successful > 0) {
                await this.loadRecentLinks();
            }

        } catch (error) {
            alert('Ошибка создания ссылок: ' + error.message);
        } finally {
            submitBtn.disabled = false;
            submitBtn.textContent = originalBtn;
        }
    },

    /**
     * Инициализация модуля Dashboard
     */
    async init() {
        await this.loadDomains();
        await this.loadRecentLinks();

        // Добавляем первое поле
        this.addLinkField();

        // Обработчик кнопки добавления поля
        const addFieldBtn = document.getElementById('addLinkFieldBtn');
        if (addFieldBtn) {
            addFieldBtn.addEventListener('click', () => this.addLinkField());
        }

        // Обработчик формы создания ссылок
        const createForm = document.getElementById('createLinksForm');
        if (createForm) {
            createForm.addEventListener('submit', (e) => this.handleCreateLinks(e));
        }
    }
};


// ============================================
// LINKS MODULE (Модуль управления ссылками)
// ============================================

const Links = {
    // Состояние
    state: {
        currentPage: 1,
        totalPages: 1,
        pageSize: 25,
        fromDate: '',
        toDate: '',
        selectedDomain: ''
    },
    domains: [],

    /**
     * Загрузка доменов
     */
    async loadDomains() {
        try {
            const data = await API.getDomains();
            this.domains = data.items || [];
            this.populateDomainFilter();
        } catch (error) {
            console.error('Ошибка загрузки доменов:', error);
        }
    },

    /**
     * Заполнение фильтра доменов
     */
    populateDomainFilter() {
        const domainSelect = document.getElementById('domainFilter');
        if (!domainSelect) return;

        domainSelect.innerHTML = '<option value="">Все домены</option>' +
            this.domains
                .filter(d => d.is_active)
                .map(d => `<option value="${d.domain}">${d.domain}</option>`)
                .join('');
    },

    /**
     * Загрузка таблицы ссылок
     */
    async loadLinks() {
        const container = document.getElementById('linksTable');
        Utils.showLoading(container);

        try {
            const params = {
                page: this.state.currentPage,
                page_size: this.state.pageSize
            };

            if (this.state.fromDate) params.from = this.state.fromDate;
            if (this.state.toDate) params.to = this.state.toDate;
            if (this.state.selectedDomain) params.domain = this.state.selectedDomain;

            const data = await API.getLinks(params);

            if (!data.items || data.items.length === 0) {
                Utils.showEmpty('Ссылки не найдены', container);
                return;
            }

            this.state.totalPages = data.pagination.total_pages || 1;

            container.innerHTML = `
                <table>
                    <thead>
                        <tr>
                            <th>Код</th>
                            <th>Домен</th>
                            <th>Оригинальная ссылка</th>
                            <th>Переходы</th>
                            <th>Создана</th>
                            <th>Действия</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${data.items.map(link => `
                            <tr>
                                <td>
                                    <a href="https://${link.domain}/${link.code}" target="_blank">
                                        <code>${link.code}</code>
                                    </a>
                                </td>
                                <td><code>${link.domain}</code></td>
                                <td style="max-width: 400px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;"
                                    title="${link.long_url}">
                                    ${link.long_url}
                                </td>
                                <td>${link.total || 0}</td>
                                <td>${Utils.formatDate(link.created_at)}</td>
                                <td>
                                    <div class="actions">
                                        <a href="/dashboard/stats/${link.code}" class="btn btn-sm">Статистика</a>
                                        <button onclick="Links.copyLink('${link.domain}', '${link.code}', this)" 
                                                class="btn btn-sm btn-secondary">📋</button>
                                    </div>
                                </td>
                            </tr>
                        `).join('')}
                    </tbody>
                </table>
                <div class="pagination-info">
                    Показано ${data.items.length} из ${data.pagination.total_items} ссылок
                </div>
            `;
            this.renderPagination();

        } catch (error) {
            Utils.showError('Ошибка загрузки ссылок: ' + error.message, container);
        }
    },

    /**
     * Копирование ссылки
     */
    async copyLink(domain, code, button) {
        const url = `https://${domain}/${code}`;
        await Utils.copyToClipboard(url, button);
    },

    /**
     * Отрисовка пагинации
     */
    renderPagination() {
        const container = document.getElementById('pagination');
        if (!container) return;

        const { currentPage, totalPages } = this.state;

        let html = '<div class="pagination">';

        // Кнопка "Предыдущая"
        html += `<button ${currentPage === 1 ? 'disabled' : ''} 
                         onclick="Links.goToPage(${currentPage - 1})">← Предыдущая</button>`;

        // Номера страниц
        for (let i = 1; i <= totalPages; i++) {
            if (i === 1 || i === totalPages || (i >= currentPage - 2 && i <= currentPage + 2)) {
                html += `<button class="${i === currentPage ? 'active' : ''}" 
                                 onclick="Links.goToPage(${i})">${i}</button>`;
            } else if (i === currentPage - 3 || i === currentPage + 3) {
                html += '<button disabled>...</button>';
            }
        }

        // Кнопка "Следующая"
        html += `<button ${currentPage === totalPages ? 'disabled' : ''} 
                         onclick="Links.goToPage(${currentPage + 1})">Следующая →</button>`;

        html += '</div>';
        container.innerHTML = html;
    },

    /**
     * Переход на страницу
     */
    async goToPage(page) {
        this.state.currentPage = page;
        await this.loadLinks();
    },

    /**
     * Применение фильтров
     */
    async applyFilters() {
        const pageSize = document.getElementById('pageSizeSelect')?.value || 25;
        const fromDate = document.getElementById('fromDate')?.value || '';
        const toDate = document.getElementById('toDate')?.value || '';
        const domain = document.getElementById('domainFilter')?.value || '';

        this.state.pageSize = parseInt(pageSize);
        this.state.fromDate = fromDate ? new Date(fromDate).toISOString() : '';
        this.state.toDate = toDate ? new Date(toDate).toISOString() : '';
        this.state.selectedDomain = domain;
        this.state.currentPage = 1;

        await this.loadLinks();
    },

    /**
     * Сброс фильтров
     */
    async resetFilters() {
        document.getElementById('fromDate').value = '';
        document.getElementById('toDate').value = '';
        document.getElementById('domainFilter').value = '';
        await this.applyFilters();
    },

    /**
     * Инициализация модуля Links
     */
    async init() {
        await this.loadDomains();
        await this.loadLinks();

        // Обработчики фильтров
        const pageSizeSelect = document.getElementById('pageSizeSelect');
        const fromDate = document.getElementById('fromDate');
        const toDate = document.getElementById('toDate');
        const domainFilter = document.getElementById('domainFilter');
        const applyBtn = document.getElementById('applyFiltersBtn');
        const resetBtn = document.getElementById('resetFiltersBtn');

        if (pageSizeSelect) pageSizeSelect.addEventListener('change', () => this.applyFilters());
        if (fromDate) fromDate.addEventListener('change', () => this.applyFilters());
        if (toDate) toDate.addEventListener('change', () => this.applyFilters());
        if (domainFilter) domainFilter.addEventListener('change', () => this.applyFilters());
        if (applyBtn) applyBtn.addEventListener('click', () => this.applyFilters());
        if (resetBtn) resetBtn.addEventListener('click', () => this.resetFilters());
    }
};


// ============================================
// STATS MODULE (Модуль статистики ссылки)
// ============================================

const Stats = {
    code: null,
    chart: null,
    allClicksData: null,
    state: {
        currentPage: 1,
        totalPages: 1,
        pageSize: 25,
        fromDate: '',
        toDate: '',
        currentPeriod: 'all'
    },

    /**
     * Установка быстрого фильтра
     */
    async setQuickFilter(period) {
        this.state.currentPeriod = period;

        // Обновляем активную кнопку
        document.querySelectorAll('.quick-filter-btn').forEach(btn => {
            btn.classList.remove('active');
        });
        document.querySelector(`[data-period="${period}"]`).classList.add('active');

        // Скрываем форму произвольного периода
        document.getElementById('customPeriodForm').style.display = 'none';

        // Рассчитываем даты
        const now = new Date();
        let from = null;

        switch(period) {
            case 'today':
                from = new Date(now.getFullYear(), now.getMonth(), now.getDate());
                break;
            case 'week':
                from = new Date(now);
                from.setDate(from.getDate() - 7);
                break;
            case 'month':
                from = new Date(now);
                from.setMonth(from.getMonth() - 1);
                break;
            case 'all':
                from = null;
                break;
        }

        this.state.fromDate = from ? from.toISOString() : '';
        this.state.toDate = now.toISOString();
        this.state.currentPage = 1;

        await this.loadLinkStats();
    },

    /**
     * Показать/скрыть форму произвольного периода
     */
    toggleCustomPeriod() {
        const form = document.getElementById('customPeriodForm');
        const isVisible = form.style.display !== 'none';

        if (isVisible) {
            form.style.display = 'none';
        } else {
            form.style.display = 'block';
            // Снимаем активность со всех кнопок
            document.querySelectorAll('.quick-filter-btn').forEach(btn => {
                btn.classList.remove('active');
            });
            document.querySelector('[data-period="custom"]').classList.add('active');
        }
    },

    /**
     * Применить произвольный период
     */
    async applyCustomPeriod() {
        const fromInput = document.getElementById('statsFromDate');
        const toInput = document.getElementById('statsToDate');

        const from = fromInput.value ? new Date(fromInput.value).toISOString() : '';
        const to = toInput.value ? new Date(toInput.value).toISOString() : '';

        if (!from && !to) {
            alert('Укажите хотя бы одну дату');
            return;
        }

        this.state.fromDate = from;
        this.state.toDate = to;
        this.state.currentPeriod = 'custom';
        this.state.currentPage = 1;

        await this.loadLinkStats();
    },

    /**
     * Загрузка информации о ссылке и кликов
     */
    async loadLinkStats() {
        try {
            // 1. Сначала загружаем данные для ТАБЛИЦЫ (с пагинацией)
            const tableParams = {
                page: this.state.currentPage,
                page_size: this.state.pageSize
            };

            if (this.state.fromDate) tableParams.from = this.state.fromDate;
            if (this.state.toDate) tableParams.to = this.state.toDate;

            const tableData = await API.getLinkStats(this.code, tableParams);

            // Информация о ссылке (только при первой загрузке)
            if (this.state.currentPage === 1) {
                const shortUrl = `https://${tableData.domain}/${tableData.code}`;
                document.getElementById('shortUrl').textContent = shortUrl;
                document.getElementById('shortUrl').href = shortUrl;
                document.getElementById('longUrl').textContent = tableData.long_url;
                document.getElementById('longUrl').href = tableData.long_url;
                document.getElementById('domain').textContent = tableData.domain;
                document.getElementById('totalClicks').textContent = tableData.total || 0;
                document.getElementById('createdAt').textContent = Utils.formatDateTime(tableData.created_at);

                // Кнопка копирования
                const copyBtn = document.getElementById('copyBtn');
                if (copyBtn) {
                    copyBtn.onclick = () => Utils.copyToClipboard(shortUrl, copyBtn);
                }

                // 2. Загружаем ВСЕ данные для ГРАФИКА (большой page_size)
                await this.loadAllClicksForChart();
            }

            // Отображаем таблицу с текущей страницей
            this.renderClicksTable(tableData);

            this.state.totalPages = tableData.pagination.total_pages || 1;
            this.renderPagination();

        } catch (error) {
            console.error('Ошибка загрузки статистики:', error);
            Utils.showError('Ошибка загрузки статистики: ' + error.message,
                document.getElementById('clicksTable'));
        }
    },

    /**
     * Загрузка ВСЕХ кликов для построения графика
     */
    async loadAllClicksForChart() {
        try {
            const chartParams = {
                page: 1,
                page_size: 1000 // Большой page_size для получения всех данных
            };

            if (this.state.fromDate) chartParams.from = this.state.fromDate;
            if (this.state.toDate) chartParams.to = this.state.toDate;

            const allData = await API.getLinkStats(this.code, chartParams);
            this.allClicksData = allData.items || [];

            // Строим график
            this.buildChartData();
            this.renderClicksChart();

        } catch (error) {
            console.error('Ошибка загрузки данных для графика:', error);
        }
    },

    /**
     * Построение данных для графика (ИСПРАВЛЕНО: учитываем временную зону)
     */
    buildChartData() {
        if (!this.allClicksData || this.allClicksData.length === 0) {
            this.chartData = [];
            return;
        }

        // Группируем клики по датам (в ЛОКАЛЬНОЙ временной зоне)
        const clicksByDate = {};
        this.allClicksData.forEach(click => {
            // Парсим дату с учетом временной зоны
            const clickDate = new Date(click.clicked_at);
            // Получаем дату в локальной временной зоне
            const localDateStr = clickDate.getFullYear() + '-' +
                String(clickDate.getMonth() + 1).padStart(2, '0') + '-' +
                String(clickDate.getDate()).padStart(2, '0');

            clicksByDate[localDateStr] = (clicksByDate[localDateStr] || 0) + 1;
        });

        // Определяем диапазон дат
        let startDate, endDate;

        if (this.state.fromDate && this.state.toDate) {
            startDate = new Date(this.state.fromDate);
            endDate = new Date(this.state.toDate);
        } else {
            // Если нет фильтра, берем последние 30 дней
            endDate = new Date();
            startDate = new Date();
            startDate.setDate(startDate.getDate() - 29);
        }

        // Заполняем все даты в диапазоне (включая дни без кликов)
        this.chartData = [];
        const currentDate = new Date(startDate);

        while (currentDate <= endDate) {
            // Формируем дату в том же формате
            const dateStr = currentDate.getFullYear() + '-' +
                String(currentDate.getMonth() + 1).padStart(2, '0') + '-' +
                String(currentDate.getDate()).padStart(2, '0');

            this.chartData.push({
                date: dateStr,
                clicks: clicksByDate[dateStr] || 0
            });
            currentDate.setDate(currentDate.getDate() + 1);
        }
    },


    /**
     * Отрисовка графика кликов (ECharts)
     */
    renderClicksChart() {
        const chartDom = document.getElementById('clicksChart');

        if (!this.chartData || this.chartData.length === 0) {
            // Если нет данных - скрываем контейнер графика
            chartDom.innerHTML = '<p style="text-align: center; color: #999; padding: 20px;">Нет данных для отображения графика</p>';
            chartDom.style.minHeight = 'auto'; // Убираем минимальную высоту
            return;
        }

        // Восстанавливаем высоту если были данные
        chartDom.style.minHeight = '400px';
        chartDom.innerHTML = ''; // Очищаем предыдущее содержимое

        // Уничтожаем предыдущий график если есть
        if (this.chart) {
            this.chart.dispose();
        }

        // Создаем новый график
        this.chart = echarts.init(chartDom);

        const option = {
            tooltip: {
                trigger: 'axis',
                backgroundColor: 'rgba(50, 50, 50, 0.9)',
                borderColor: '#ccc',
                textStyle: { color: '#fff' }
            },
            grid: {
                left: '3%',
                right: '3%',
                top: '5%',
                bottom: '10%',
                containLabel: true
            },
            xAxis: {
                type: 'category',
                data: this.chartData.map(d => {
                    const date = new Date(d.date);
                    return date.toLocaleDateString('ru-RU', { month: 'short', day: 'numeric' });
                }),
                boundaryGap: false
            },
            yAxis: {
                type: 'value',
                minInterval: 1
            },
            series: [
                {
                    name: 'Клики',
                    type: 'line',
                    data: this.chartData.map(d => d.clicks),
                    smooth: true,
                    itemStyle: {
                        color: '#2563eb'
                    },
                    areaStyle: {
                        color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
                            {
                                offset: 0,
                                color: 'rgba(37, 99, 235, 0.3)'
                            },
                            {
                                offset: 1,
                                color: 'rgba(37, 99, 235, 0.1)'
                            }
                        ])
                    },
                    lineStyle: {
                        color: '#2563eb',
                        width: 2
                    }
                }
            ]
        };

        this.chart.setOption(option);

        // Адаптивность при изменении размера окна
        const resizeHandler = () => {
            if (this.chart) {
                this.chart.resize();
            }
        };

        window.removeEventListener('resize', resizeHandler);
        window.addEventListener('resize', resizeHandler);
    },

    /**
     * Отрисовка таблицы кликов
     */
    renderClicksTable(data) {
        const container = document.getElementById('clicksTable');

        if (!data.items || data.items.length === 0) {
            Utils.showEmpty('Нет данных о кликах за выбранный период', container);
            return;
        }

        container.innerHTML = `
            <table>
                <thead>
                    <tr>
                        <th>Дата и время</th>
                        <th>User Agent</th>
                        <th>Источник (Referer)</th>
                        <th>IP адрес</th>
                    </tr>
                </thead>
                <tbody>
                    ${data.items.map(item => `
                        <tr>
                            <td>${Utils.formatDateTime(item.clicked_at)}</td>
                            <td style="max-width: 300px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;"
                                title="${item.user_agent || '—'}">
                                ${item.user_agent || '—'}
                            </td>
                            <td style="max-width: 300px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;"
                                title="${item.referer || '—'}">
                                ${item.referer ? `<a href="${item.referer}" target="_blank">${item.referer}</a>` : '—'}
                            </td>
                            <td><code>${item.ip || '—'}</code></td>
                        </tr>
                    `).join('')}
                </tbody>
            </table>
            <div class="pagination-info">
                Показано ${data.items.length} из ${data.pagination.total_items} кликов за выбранный период
            </div>
        `;
    },

    /**
     * Отрисовка пагинации
     */
    renderPagination() {
        const container = document.getElementById('pagination');
        if (!container) return;

        const { currentPage, totalPages } = this.state;

        let html = '<div class="pagination">';

        html += `<button ${currentPage === 1 ? 'disabled' : ''} 
                         onclick="Stats.goToPage(${currentPage - 1})">← Предыдущая</button>`;

        for (let i = 1; i <= totalPages; i++) {
            if (i === 1 || i === totalPages || (i >= currentPage - 2 && i <= currentPage + 2)) {
                html += `<button class="${i === currentPage ? 'active' : ''}" 
                                 onclick="Stats.goToPage(${i})">${i}</button>`;
            } else if (i === currentPage - 3 || i === currentPage + 3) {
                html += '<button disabled>...</button>';
            }
        }

        html += `<button ${currentPage === totalPages ? 'disabled' : ''} 
                         onclick="Stats.goToPage(${currentPage + 1})">Следующая →</button>`;

        html += '</div>';
        container.innerHTML = html;
    },

    /**
     * Переход на страницу
     */
    async goToPage(page) {
        this.state.currentPage = page;
        await this.loadLinkStats();
    },

    /**
     * Инициализация модуля Stats
     */
    async init(code) {
        this.code = code;
        await this.setQuickFilter('all');
    }
};

/**
 * ============================================
 * LOGIN MODULE
 * ============================================
 */
let Login = {
    init() {
        const loginForm = document.getElementById('loginForm');
        if (!loginForm) return;
        loginForm.addEventListener('submit', this.handleLogin.bind(this));
    },

    async handleLogin(e) {
        e.preventDefault();

        const token = document.getElementById('token').value;
        const errorDiv = document.getElementById('error');
        const submitBtn = e.target.querySelector('button[type="submit"]');

        submitBtn.disabled = true;
        submitBtn.textContent = 'Проверка...';
        errorDiv.style.display = 'none';

        try {
            const response = await fetch('/api/health/', {
                headers: {
                    'Authorization': `Bearer ${token}`
                }
            });

            if (response.ok) {
                // Сохраняем токен ТОЛЬКО в cookie
                document.cookie = `auth_token=${token}; path=/; max-age=2592000; SameSite=Strict`;

                // Перенаправляем на dashboard
                window.location.href = '/dashboard';
                return;
            }

            const errorMessage = response.status === 401
                ? 'Неверный токен'
                : 'Ошибка проверки токена';

            this.showError(errorDiv, submitBtn, errorMessage);

        } catch (error) {
            this.showError(errorDiv, submitBtn, 'Ошибка сети: ' + error.message);
        }
    },

    showError(errorDiv, submitBtn, message) {
        errorDiv.textContent = '❌ ' + message;
        errorDiv.style.display = 'block';
        submitBtn.disabled = false;
        submitBtn.textContent = 'Войти';
    }
};

// ============================================
// ЭКСПОРТ В ГЛОБАЛЬНУЮ ОБЛАСТЬ
// ============================================

window.Auth = Auth;
window.Utils = Utils;
window.API = API;
window.Dashboard = Dashboard;
window.Links = Links;
window.Stats = Stats;
window.Login = Login;
