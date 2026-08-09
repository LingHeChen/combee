import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import CreateCellPage from "@/app/[locale]/cells/new/page";
import { I18nProvider } from "@/lib/i18n-context";
import { KvBrowser } from "@/components/kv-browser";

function W({ children }: { children: React.ReactNode }) {
  return <I18nProvider locale="zh">{children}</I18nProvider>;
}

const { pushMock } = vi.hoisted(() => ({ pushMock: vi.fn() }));
const bffFetchMock = vi.hoisted(() => vi.fn());

// mock next/navigation
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => "/cells/new",
  useParams: () => ({ locale: "zh" }),
}));

// mock BFF(避免真实 fetch)
vi.mock("@/lib/bff/client", () => ({
  bffFetch: bffFetchMock,
}));

describe("CreateCellPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    bffFetchMock.mockReset();
    bffFetchMock.mockResolvedValue({ id: "mock-cell-1" });
  });

  it("renders the form", async () => {
    render(<W><CreateCellPage /></W>);
    expect(await screen.findByTestId("create-cell-form")).toBeInTheDocument();
    expect(screen.getByTestId("create-cell-submit")).toBeInTheDocument();
  });

  it("submits and navigates on create (ensure by name)", async () => {
    bffFetchMock.mockResolvedValue({ cell: { id: "mock-cell-1" }, created: true });
    render(<W><CreateCellPage /></W>);
    fireEvent.change(screen.getByTestId("cell-name-input"), { target: { value: "my-app" } });
    fireEvent.click(screen.getByTestId("create-cell-submit"));
    await waitFor(() =>
      expect(bffFetchMock).toHaveBeenCalledWith("/v1/databases/by-name/my-app", { method: "PUT" }),
    );
    await waitFor(() => expect(pushMock).toHaveBeenCalledWith("/zh/cells/mock-cell-1"));
  });

  it("requires a name (ensure semantics)", async () => {
    render(<W><CreateCellPage /></W>);
    fireEvent.click(screen.getByTestId("create-cell-submit"));
    await waitFor(() => expect(screen.getByText("请输入名称")).toBeTruthy());
    expect(bffFetchMock).not.toHaveBeenCalled();
  });
});

describe("KvBrowser", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    bffFetchMock.mockReset();
  });

  it("renders browse mode and switches to operate", () => {
    render(<W><KvBrowser cellId="c1" /></W>);
    expect(screen.getByTestId("kv-mode-browse")).toBeInTheDocument();
    expect(screen.getByTestId("kv-browse")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("kv-mode-operate"));
    expect(screen.getByTestId("kv-key")).toBeInTheDocument();
    expect(screen.getByTestId("kv-value")).toBeInTheDocument();
    expect(screen.getByTestId("kv-ttl")).toBeInTheDocument();
    expect(screen.getByTestId("kv-get")).toBeInTheDocument();
    expect(screen.getByTestId("kv-set")).toBeInTheDocument();
    expect(screen.getByTestId("kv-del")).toBeInTheDocument();
  });

  it("set calls the real KV API", async () => {
    bffFetchMock.mockResolvedValue({ written: true });
    render(<W><KvBrowser cellId="c1" /></W>);
    fireEvent.click(screen.getByTestId("kv-mode-operate"));
    fireEvent.change(screen.getByTestId("kv-key"), { target: { value: "user:1" } });
    fireEvent.change(screen.getByTestId("kv-value"), { target: { value: "v1" } });
    fireEvent.click(screen.getByTestId("kv-set"));
    await waitFor(() =>
      expect(bffFetchMock).toHaveBeenCalledWith("/v1/databases/c1/kv/user%3A1", {
        method: "PUT",
        body: { value: "v1" },
      }),
    );
    await waitFor(() => expect(screen.getByTestId("kv-result").textContent).toContain("✓"));
  });

  it("get requires a key", async () => {
    render(<W><KvBrowser cellId="c1" /></W>);
    fireEvent.click(screen.getByTestId("kv-mode-operate"));
    fireEvent.click(screen.getByTestId("kv-get"));
    await waitFor(() => expect(screen.getByTestId("kv-result").textContent).toContain("请输入 Key"));
  });
});
