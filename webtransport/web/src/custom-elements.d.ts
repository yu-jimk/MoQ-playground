// @moq/publish, @moq/watch が登録するカスタム要素を JSX で使えるようにする型宣言。
// url / name / connection はプロパティで設定する(コンポーネント側で ref 経由)ため、
// ここでは属性(source など)と children/ref だけ許可する。
import type { DetailedHTMLProps, HTMLAttributes } from "react";

type CustomElement = DetailedHTMLProps<HTMLAttributes<HTMLElement>, HTMLElement>;

declare module "react" {
  namespace JSX {
    interface IntrinsicElements {
      "moq-publish": CustomElement & { source?: string };
      "moq-watch": CustomElement;
    }
  }
}
