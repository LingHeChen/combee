"use client";

import { useState } from "react";
import { type Dict } from "@/lib/i18n";
import { WAITLIST_URL } from "@/lib/config";

/** Public Beta 候补登记:邮箱写入 Combee(公开 /v1/waitlist,幂等)。 */
export function WaitlistForm({ t }: { t: Dict }) {
  const [email, setEmail] = useState("");
  const [status, setStatus] = useState<"idle" | "busy" | "ok" | "err">("idle");
  const [msg, setMsg] = useState("");

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    const value = email.trim();
    if (!/^[^@\s]+@[^@\s]+\.[^@\s]{2,}$/.test(value)) {
      setStatus("err");
      setMsg(t.waitlist.invalid);
      return;
    }
    setStatus("busy");
    try {
      const res = await fetch(WAITLIST_URL, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ email: value }),
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      setStatus("ok");
      setMsg(t.waitlist.success);
      setEmail("");
    } catch {
      setStatus("err");
      setMsg(t.waitlist.error);
    }
  }

  return (
    <form onSubmit={onSubmit} className="mt-5 flex flex-col gap-2" data-testid="waitlist-form">
      <label htmlFor="waitlist-email" className="mono-label">
        {t.waitlist.label}
      </label>
      <div className="flex gap-2">
        <input
          id="waitlist-email"
          data-testid="waitlist-email"
          type="email"
          required
          placeholder={t.waitlist.placeholder}
          value={email}
          onChange={(e) => {
            setEmail(e.target.value);
            setStatus("idle");
          }}
          className="flex-1 min-w-0 rounded-[4px] border border-[#1f2937] bg-[#0a0a0a] px-3 py-2.5 font-mono-code text-sm text-[#fafaf9] placeholder:text-[#6b7280] focus:border-[#f59e0b] focus:outline-none"
        />
        <button
          type="submit"
          disabled={status === "busy"}
          data-testid="waitlist-submit"
          className="btn-primary px-4 py-2.5 text-sm whitespace-nowrap disabled:opacity-60"
        >
          {status === "busy" ? "…" : t.waitlist.submit}
        </button>
      </div>
      {status === "ok" && (
        <p className="font-mono-label text-[#f59e0b] text-xs" data-testid="waitlist-ok">
          {msg}
        </p>
      )}
      {status === "err" && (
        <p className="font-mono-label text-[#ffb4ab] text-xs" data-testid="waitlist-err">
          {msg}
        </p>
      )}
      <p className="font-mono-label text-[#c4c7c7]/70 text-[10px]">{t.waitlist.hint}</p>
    </form>
  );
}
