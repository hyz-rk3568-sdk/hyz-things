---
tags:
  - 资料
  - 年均变化
  - 年均增速
  - 公务员考试
---

## Card
<!-- hyz-card-id: 01a111cc-7f48-7587-a30b-1cbe87508f23 -->

### Front

年均变化题，基期怎么选？非江苏、江苏、五年计划分别是什么？

### Back

- **非江苏普通区间**：首年年末作基期，$n=\text{末年}-\text{首年}$。
- **江苏**：首年再前推一年作基期，$n=\text{末年}-\text{首年}+1$。
- **五年计划**：从第一年第一秒到第五年最后一秒，基期取第一年前一年年末，$n=5$。

---

## Card
<!-- hyz-card-id: 01a111cc-7f48-7587-a30b-1cbf2e4fa84a -->

### Front

保持**增量**不变，何时追上另一个值的 $\frac{1}{n}$？

### Back

追赶公式仍是：

$$
\boxed{T=\frac{\Delta L}{\Delta V}}
$$

若追者为 $A+at$，目标为另一值 $B+bt$ 的 $\frac1n$，则：

$$
\boxed{n(A+at)=B+bt}
$$

即：**把追者的现期量、增量同时乘 $n$，再按普通追赶题算。**

---

## Card
<!-- hyz-card-id: 01a111cc-7f48-7587-a30b-1cc04a40c9da -->

### Front

年均增速的核心公式和二项式展开是什么？

### Back

设年均增速为 $r$，总增长率为 $r_{\text{总}}$：

$$
\boxed{(1+r)^n=\frac{\text{现期}}{\text{基期}}=1+r_{\text{总}}}
$$

二项式展开：

$$
\boxed{r_{\text{总}}=nr+\binom{n}{2}r^2+\binom{n}{3}r^3+\cdots+r^n}
$$

常用降次：

$$
\boxed{
\begin{aligned}
n=3:&\quad r_{\text{总}}\approx3r+3r^2 \\
n=4:&\quad r_{\text{总}}\approx4r+6r^2 \\
n=5:&\quad r_{\text{总}}\approx5r+10r^2
\end{aligned}
}
$$

---

## Card
<!-- hyz-card-id: 01a111cc-7f49-7734-9d23-68d2c9e2ff1e -->

### Front

年均增长率和算术平均增长率怎么比较？为什么？

### Back

算术平均增长率：

$$
\boxed{r_{\text{算术}}=\frac{r_1+r_2+\cdots+r_n}{n}}
$$

各年增速不完全相同时：

$$
\boxed{r_{\min}<r_{\text{年均}}<r_{\text{算术}}<r_{\max}}
$$

原理：**和定积最**。各数和固定时，越接近，乘积越大。

因此各年增速越接近：

$$
\boxed{r_{\text{年均}}\text{ 越接近 }r_{\text{算术}}}
$$

---

## Card
<!-- hyz-card-id: 01a111cc-7f49-7734-9d23-68d3937447ed -->

### Front

假设降次法如何估年均增速？

### Back

先保留二次项：

$$
\boxed{nr+\binom{n}{2}r^2\approx r_{\text{总}}}
$$

不要直接解二次方程。

先取一个假设值：

$$
\boxed{r_0\approx\frac{r_{\text{总}}}{n}}
$$

或直接取靠近的选项，只把 $r^2$ 用 $r_0^2$ 代入：

$$
\boxed{r\approx\frac{r_{\text{总}}-\binom{n}{2}r_0^2}{n}}
$$

核心：**假设平方项，降成一次计算。**

---

## Card
<!-- hyz-card-id: 01a111cc-7f49-7734-9d23-68d4c8d7dba4 -->

### Front

年均增速误差较小时，怎么快速估？什么时候该用代入验证？

### Back

当年均增速绝对值较小（讲义取 $<20\%$）：

$$
\boxed{r_{\text{年均}}\approx\frac{r_{\text{总}}}{n}-\left(\frac{r_{\text{总}}}{n}\right)^2}
$$

- $n=3$：**向上取**
- $n=4,5,\ldots$：**向下取**

当：

$$
\boxed{|r_{\text{年均}}|\ge20\%}
$$

取选项中间的整值代入验证。

如 $48\%/52\%$ 取 $50\%$；$31\%/35\%$ 可取 $\frac13$。

$(1 + 0.1)^4 = {1.21}^2$

---

## Card
<!-- hyz-card-id: 01a111cc-7f49-7734-9d23-68d52b72d550 -->

### Front

年均增长率比较大小，最快怎么判断？

### Back

抓住：

$$
\boxed{(1+r_{\text{年均}})^n=1+r_{\text{总}}}
$$

- **年数 $n$ 相同**：直接比较 $r_{\text{总}}$，总增长率越大，年均增速越大。
- 也可直接比较：

$$
\boxed{\frac{\text{现期}}{\text{基期}}}
$$

- **年数不同**：不能直接比总增长率，要结合 $n$，用 $(1+r)^n$ 代入判断。

---
