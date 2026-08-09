import { describe, it, expect } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import CodeBlock from "./code-block";

// prismjs 组件在 node/jsdom 可加载
describe("CodeBlock", () => {
  it("渲染标题与代码文本", () => {
    render(<CodeBlock code="SELECT 1" language="sql" title="sql" />);
    expect(screen.getByText("sql")).toBeTruthy();
    expect(screen.getByText("SELECT 1")).toBeTruthy();
  });

  it("异步高亮后包含 token 结构(SQL 关键字被着色)", async () => {
    render(<CodeBlock code="SELECT * FROM users WHERE id = 1" language="sql" />);
    await waitFor(
      () => {
        const codeEl = document.querySelector("code.language-sql");
        expect(codeEl?.querySelector(".token.keyword")).toBeTruthy();
      },
      { timeout: 3000 },
    );
  });

  it("TS 代码高亮含 keyword/string token", async () => {
    render(
      <CodeBlock
        code={'const cell = await combee.cells.create({ name: "my-app" });'}
        language="typescript"
      />,
    );
    await waitFor(
      () => {
        const codeEl = document.querySelector("code.language-typescript");
        expect(codeEl?.querySelector(".token.keyword")).toBeTruthy();
        expect(codeEl?.querySelector(".token.string")).toBeTruthy();
      },
      { timeout: 3000 },
    );
  });
});
