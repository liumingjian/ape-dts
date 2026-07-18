import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { createI18n } from "vue-i18n";
import { createMemoryHistory, createRouter } from "vue-router";
import { defineComponent, h } from "vue";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ElMessage } from "element-plus";
import { api } from "@/api/client";
import payloadContract from "../fixtures/checkStructWizardPayloadContract.json";

vi.mock("@/api/client", () => ({
  api: {
    get: vi.fn().mockResolvedValue([]),
    post: vi.fn().mockResolvedValue({}),
    put: vi.fn().mockResolvedValue({}),
    del: vi.fn().mockResolvedValue({}),
  },
}));

const Stub = defineComponent({ name: "Stub", render: () => h("div") });
const i18n = createI18n({
  legacy: false,
  locale: "en-US",
  fallbackLocale: "en-US",
  messages: { "en-US": {}, "zh-CN": {} },
  missingWarn: false,
  fallbackWarn: false,
});

async function mountWizard(kind: "check" | "struct") {
  setActivePinia(createPinia());
  const Wizard = (await import("@/views/tasks/CreateTaskWizard.vue")).default;
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: "/tasks/create/:type", component: Wizard },
      { path: "/tasks/:category/:id", component: Stub },
    ],
  });
  await router.push(`/tasks/create/${kind}`);
  await router.isReady();
  const wrapper = mount(Wizard, {
    global: {
      plugins: [router, i18n],
      stubs: { ConnectionTestCard: Stub, EngineTag: Stub },
    },
  });
  await flushPromises();
  return { router, wrapper };
}

function exposed(wrapper: Awaited<ReturnType<typeof mountWizard>>["wrapper"]) {
  const setupState = (
    wrapper.vm.$ as unknown as {
      setupState: {
        form: { filter: { doDbs: string; doTbs: string } };
        runTest: (which: "source" | "target") => Promise<void>;
        runPrecheck: () => Promise<void>;
        togglePreview: () => Promise<void>;
      };
    }
  ).setupState;
  return {
    onSubmit: (wrapper.vm as unknown as { onSubmit: () => Promise<void> }).onSubmit,
    ...setupState,
  };
}

describe("Check and Struct wizard backend boundary", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.mocked(api.get).mockReset().mockResolvedValue([]);
    vi.mocked(api.post).mockReset().mockResolvedValue({});
    vi.spyOn(ElMessage, "success").mockImplementation(() => undefined as never);
    vi.spyOn(ElMessage, "warning").mockImplementation(() => undefined as never);
    vi.spyOn(ElMessage, "error").mockImplementation(() => undefined as never);
  });

  it("submits Check with the backend-required sandboxed check log sinker", async () => {
    vi.mocked(api.post).mockResolvedValueOnce({ id: "check-42", category: "check" });
    const { router, wrapper } = await mountWizard("check");

    await exposed(wrapper).onSubmit();
    await flushPromises();

    const creates = vi.mocked(api.post).mock.calls.filter(([url]) => url === "/tasks");
    expect(creates).toHaveLength(1);
    expect(creates[0]?.[1]).toMatchObject(payloadContract.check);
    expect(router.currentRoute.value.fullPath).toBe("/tasks/check/check-42");
  });

  it("submits Struct with non-empty array filters accepted by backend validation", async () => {
    vi.mocked(api.post).mockResolvedValueOnce({ id: "struct-42", category: "struct" });
    const { router, wrapper } = await mountWizard("struct");

    await exposed(wrapper).onSubmit();
    await flushPromises();

    const creates = vi.mocked(api.post).mock.calls.filter(([url]) => url === "/tasks");
    expect(creates).toHaveLength(1);
    expect(creates[0]?.[1]).toMatchObject(payloadContract.struct);
    expect(router.currentRoute.value.fullPath).toBe("/tasks/struct/struct-42");
  });

  it("keeps both Struct filter arrays non-empty when only tables are selected", async () => {
    vi.mocked(api.post).mockResolvedValueOnce({ id: "struct-43", category: "struct" });
    const { wrapper } = await mountWizard("struct");
    const wizard = exposed(wrapper);
    wizard.form.filter.doDbs = "";
    wizard.form.filter.doTbs = "sales.orders";

    await wizard.onSubmit();
    await flushPromises();

    const createBody = vi.mocked(api.post).mock.calls.find(
      ([url]) => url === "/tasks",
    )?.[1] as { filter: { do_dbs: string[]; do_tbs: string[] } };
    expect(createBody.filter.do_dbs).toEqual(["*"]);
    expect(createBody.filter.do_tbs).toEqual(["sales.orders"]);
  });

  it("uses draft endpoints for connection test, precheck, and INI preview without creating a task", async () => {
    vi.mocked(api.post).mockImplementation(async (url: string) => {
      if (url === "/tasks/preview/test_connection") {
        return { source: { ok: true }, target: { ok: true }, requestId: "req-test" };
      }
      if (url === "/tasks/preview/precheck") {
        return { items: [], requestId: "req-precheck" };
      }
      if (url === "/tasks/preview-ini") return { ini: "[sinker]\nsink_type=check" };
      return {};
    });
    const { wrapper } = await mountWizard("check");

    await exposed(wrapper).runTest("source");
    await exposed(wrapper).runPrecheck();
    await exposed(wrapper).togglePreview();

    const urls = vi.mocked(api.post).mock.calls.map(([url]) => url);
    expect(urls).toContain("/tasks/preview/test_connection");
    expect(urls).toContain("/tasks/preview/precheck");
    expect(urls).toContain("/tasks/preview-ini");
    expect(urls).not.toContain("/tasks");
  });

  it("uses the exact Check DTO for Confirm preview and submit", async () => {
    vi.mocked(api.post)
      .mockResolvedValueOnce({ ini: "[sinker]\nsink_type=check\ncheck_log_dir=./check" })
      .mockResolvedValueOnce({ id: "check-preview", category: "check" })
      .mockResolvedValueOnce({});
    const { wrapper } = await mountWizard("check");

    await exposed(wrapper).togglePreview();
    await exposed(wrapper).onSubmit();
    await flushPromises();

    const previewBody = vi.mocked(api.post).mock.calls.find(
      ([url]) => url === "/tasks/preview-ini",
    )?.[1];
    const createBody = vi.mocked(api.post).mock.calls.find(
      ([url]) => url === "/tasks",
    )?.[1];
    expect(previewBody).toEqual(createBody);
  });

  it("starts only after create succeeds and reports an explicit partial outcome when start fails", async () => {
    vi.mocked(api.post)
      .mockResolvedValueOnce({ id: "check-43", category: "check" })
      .mockRejectedValueOnce(new Error("runner unavailable"));
    const { router, wrapper } = await mountWizard("check");

    await exposed(wrapper).onSubmit();
    await flushPromises();

    expect(vi.mocked(api.post).mock.calls.slice(-2).map(([url]) => url)).toEqual([
      "/tasks",
      "/tasks/check-43/start",
    ]);
    expect(ElMessage.warning).toHaveBeenCalledWith(
      expect.stringContaining("runner unavailable"),
    );
    expect(ElMessage.success).not.toHaveBeenCalled();
    expect(router.currentRoute.value.fullPath).toBe("/tasks/check/check-43");
  });

  it("does not start or redirect when creation fails", async () => {
    vi.mocked(api.post).mockRejectedValueOnce(new Error("validation failed"));
    const { router, wrapper } = await mountWizard("struct");

    await exposed(wrapper).onSubmit();
    await flushPromises();

    expect(vi.mocked(api.post).mock.calls.map(([url]) => url)).toEqual(["/tasks"]);
    expect(ElMessage.success).not.toHaveBeenCalled();
    expect(router.currentRoute.value.fullPath).toBe("/tasks/create/struct");
  });
});
