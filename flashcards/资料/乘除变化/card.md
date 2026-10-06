---
tags:
  - 资料
  - 乘除变化
  - 公务员考试
---

## Card
<!-- hyz-card-id: 01a110d8-d5b5-7561-8765-e18c2f2a72ec -->

### Front

增长率混合运算与比较的常用公式：

- **乘**：$(1+r_1)(1+r_2)$
- **除**：$\dfrac{1+r_1}{1+r_2}=1+\dfrac{r_1-r_2}{1+r_2}$
- **基期比**：$\dfrac{A_{\text{基}}}{B_{\text{基}}}=\dfrac{A}{B}\times\dfrac{1+r_B}{1+r_A}$
- **增量比**：$\dfrac{\Delta A}{\Delta B}=\dfrac{A}{B}\times\dfrac{r_A}{r_B}\times\dfrac{1+r_B}{1+r_A}$
- **基期比重**：$P_{\text{基}}=P_{\text{现}}\times\dfrac{1+r_B}{1+r_A}$
- **两期比重差**：$P_{\text{现}}-P_{\text{基}}=P_{\text{现}}\times\dfrac{r_A-r_B}{1+r_A}$

### Back

设 $A、B$ 为现期量，增长率分别为 $r_A、r_B$，且 $r_A、r_B>-1$。

## 1. 增长率的乘除

### 乘

$$
\boxed{
(1+r_1)(1+r_2)
=1+r_1+r_2+r_1r_2
}
$$

### 除

$$
\boxed{
\frac{1+r_1}{1+r_2}
=1+\frac{r_1-r_2}{1+r_2}
}
$$

## 2. 基期比较大小

设 $A、B$ 为现期量，增长率分别为 $r_A、r_B$：

$$
\begin{aligned}
\frac{A_{\text{基}}}{B_{\text{基}}}
&=\frac{\frac{A}{1+r_A}}{\frac{B}{1+r_B}} \\
&=\frac{A}{B}\times\frac{1+r_B}{1+r_A}.
\end{aligned}
$$

记忆：

> 基期比 = 现期比 × 增长率修正项

$$
\boxed{
\frac{A_{\text{基}}}{B_{\text{基}}}
=\frac{A}{B}\times\frac{1+r_B}{1+r_A}
}
$$

---

## 3. 增量比较大小

单个增量：

$$
\begin{aligned}
\Delta A
&=A_{\text{基}}\times r_A \\
&=\frac{A}{1+r_A}\times r_A \\
&=\frac{Ar_A}{1+r_A}.
\end{aligned}
$$

当 $r_B\ne0$ 时，两个增量之比为

$$
\begin{aligned}
\frac{\Delta A}{\Delta B}
&=\frac{\frac{Ar_A}{1+r_A}}{\frac{Br_B}{1+r_B}} \\
&=\frac{A}{B}\times\frac{r_A}{r_B}\times\frac{1+r_B}{1+r_A}.
\end{aligned}
$$

记忆：

> 增量比 = 现期比 × 增速比 × 增长率修正项

$$
\boxed{
\frac{\Delta A}{\Delta B}
=\frac{A}{B}\times\frac{r_A}{r_B}\times\frac{1+r_B}{1+r_A}
}
$$

---

## 4. 比重

设 $A$ 为部分量，$B$ 为总体量，增长率分别为 $r_A、r_B$。

### 现期比重

$$
\boxed{
P_{\text{现}}=\frac{A}{B}
}
$$

### 基期比重

$$
\begin{aligned}
P_{\text{基}}
&=\frac{\frac{A}{1+r_A}}{\frac{B}{1+r_B}} \\
&=\frac{A}{B}\times\frac{1+r_B}{1+r_A} \\
&=P_{\text{现}}\times\frac{1+r_B}{1+r_A}.
\end{aligned}
$$

记忆：

> 基期比重 = 现期比重 × 增长率修正项

### 两期比重差

令

$$
\Delta P=P_{\text{现}}-P_{\text{基}}.
$$

则

$$
\begin{aligned}
\Delta P
&=P_{\text{现}}-P_{\text{现}}\times\frac{1+r_B}{1+r_A} \\
&=P_{\text{现}}\times\left(1-\frac{1+r_B}{1+r_A}\right) \\
&=P_{\text{现}}\times\frac{r_A-r_B}{1+r_A}.
\end{aligned}
$$

即：

$$
\boxed{
P_{\text{现}}-P_{\text{基}}
=\frac{(r_A-r_B)\times P_{\text{现}}}{1+r_A}
}
$$

方向判断：

$$
\boxed{
\operatorname{sgn}(P_{\text{现}}-P_{\text{基}})
=\operatorname{sgn}(r_A-r_B)
}
$$

- $r_A>r_B$：比重上升
- $r_A<r_B$：比重下降
- $r_A=r_B$：比重不变

---
