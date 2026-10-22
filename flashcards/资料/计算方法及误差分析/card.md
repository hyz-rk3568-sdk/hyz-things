---
tags:
  - 资料
  - 计算方法及误差分析
  - 公务员考试
---

## Card
<!-- hyz-card-id: 01a15a41-a38e-7d90-8100-ce062201b3fa -->

### Front

- 一边做题一边建立数据地图
- 放弃一步得到结果的想法，转向分步相加，10%以下用化除为乘，算基期可以先算增量，不列复杂等式
- 平均数增速和增量都是分子增速-分母增速
- 先看选项，在找数
- 第三位不同时(误差最大1%)，放缩可以用，但是不要抄原数，放缩后被除数应该只有1位，不然不要用
- 第三位不同时可以选择一个选项或选项中间值往里代
- 截掉了几位划记下，数量级要清楚，去掉了几位?
- 乘除中间结果还是要写到草稿上，尾数0要补，比如 73 * 23 = 73 * （20 + 3）= 1460 + 219 = 1679
- 脑容量是有限的，做记号就做记号，但不要抄数
- 不要清空草稿

### Back

--

---

## Card
<!-- hyz-card-id: 01a10fb8-d11b-75dd-aaf6-ab4a762b7bbb -->

### Front

- 放缩法

### Back

## 乘法放缩 

$$
\begin{aligned}
378 \times 738
&= (378 + 22) \times (738 - 38) \\
&= 280000
\end{aligned}
$$

# $\frac{118}{966}$ 计算

### 1. 补 $0$，缩小分子分母倍数差距

换算成百分数：

$$
\frac{118}{966}\times100\% = \frac{1180}{966}\times10\%.
$$

### 2. 近似计算

$$
\frac{1180+?}{966+33}
$$

因为：

$$
\frac{1180}{966}\approx\frac{11}{9}
$$

且：

$$
33=3\times9+6
$$

所以：

$$
?=3\times11+6=39
$$

于是：

$$
\begin{aligned}
\frac{118}{966}\times100\%
&\approx\frac{1180+39}{966+33}\times10\% \\
&=\frac{1219}{999}\times10\% \\
&\approx12.20\%.
\end{aligned}
$$

实际：

$$
\frac{118}{966}\times100\%\approx12.22\%.
$$

---

### 另一种近似（计算 $\frac{112}{966}$）

$$
\begin{aligned}
\frac{112}{966}\times100\%
&=\frac{1120}{966}\times10\% \\
&\approx\frac{11}{9.5}\times10\% \\
&=\frac{22}{19}\times10\%.
\end{aligned}
$$

所以：

$$
\frac{1120+?}{966+33}
$$

因为：

$$
33=2\times19-5
$$

所以：

$$
?=2\times22-5=39
$$

于是：

$$
\begin{aligned}
\frac{112}{966}\times100\%
&\approx\frac{1120+39}{966+33}\times10\% \\
&=\frac{1159}{999}\times10\% \\
&\approx11.60\%.
\end{aligned}
$$

实际：

$$
\frac{112}{966}\times100\%\approx11.59\%.
$$

---

## Card
<!-- hyz-card-id: 01a11097-8e8e-7133-bfec-e6099796c086 -->

### Front

- 化除为乘

### Back

化除为乘一般在基期量误差较小，且转换为增长量后误差可接受时使用。

当增速小于 10% 时，误差在 1% 以下。以将 $\frac{x}{99}$ 近似为 $\frac{x}{100}$ 为例：

$$
\varepsilon
= \frac{\frac{x}{99} - \frac{x}{100}}{\frac{x}{99}}
= \frac{1}{100}
= 1\%.
$$

---

## Card
<!-- hyz-card-id: 01a11097-8e8e-7133-bfec-e60a9f7e5e7d -->

### Front

- 份数法误差

### Back

设现期量为 $A$，增长率为 $r$，选取与 $r$ 接近的份数 $n$，使

$$
r \approx \frac{1}{n}.
$$

#### 1. 准确公式

增长量的准确值为

$$
\boxed{\Delta_{\text{准确}} = \frac{Ar}{1+r}}.
$$

当 $r=\frac{1}{n}$ 时，

$$
\begin{aligned}
\Delta_{\text{准确}}
&= \frac{A\cdot\frac{1}{n}}{1+\frac{1}{n}} \\
&= \frac{A}{n+1}.
\end{aligned}
$$

#### 2. 份数法估算

先用

$$
\boxed{\Delta_0 = \frac{A}{n+1}}
$$

进行估算。

#### 3. 修正公式

当 $r$ 与 $\frac{1}{n}$ 不完全相等时，用 $nr$ 修正：

$$
\boxed{
\begin{aligned}
\Delta_{\text{修}}
&= \frac{A(\frac{1}{n} - \frac{1}{n} + r)}{1 + \frac{1}{n} - \frac{1}{n} + r} \\
&= \Delta_0 \times nr \\
&= \frac{A}{n+1}\times nr \\
&= \frac{Anr}{n+1} \\
&= \frac{Ar}{1+\frac{1}{n}}.
\end{aligned}
}
$$

这相当于把准确公式中的分母 $1+r$ 近似为 $1+\frac{1}{n}$。

#### 4. 修正方向

$$
\boxed{
\begin{cases}
 nr=1 & \text{无需修正} \\
 nr<1 & \text{向下修正} \\
 nr>1 & \text{向上修正}
\end{cases}
}
$$

例如：

$$
r=18\%,\qquad \frac{1}{n}=20\% \Rightarrow n=5,
$$

则

$$
nr=5\times18\%=90\%=0.9.
$$

如果份数法初步估算结果为 $100$，则

$$
\Delta_{\text{修}}=100\times0.9=90.
$$

#### 5. 相对误差

修正值相对于准确值的带符号相对误差为

$$
\boxed{
\varepsilon
= \frac{\Delta_{\text{修}}-\Delta_{\text{准确}}}{\Delta_{\text{准确}}}
= \frac{r-\frac{1}{n}}{1+\frac{1}{n}}.
}
$$

---

## Card

<!-- hyz-card-id: 01a11097-8e8e-7133-bfec-e60bf51af714 -->

### Front

- 截位

### Back

- 保留 2 位有效数字：误差约为 1%–10%
- 保留 3 位有效数字：误差约为 0.1%–1%
- 保留 4 位有效数字：误差低于 0.1%

---

## Card
<!-- hyz-card-id: 01a14071-24c5-7a72-925d-53c5f01e251e -->

### Front

除法试商

$$
\frac{457}{277} = ?
$$

$457 - 1 \times 277 = 180$, $300 * 0.6 = 180$，所以$? = 1.6+$

### Back

--

---
