---
tags:
  - 资料
  - 增长贡献率和拉动增长率
  - 公务员考试
---

## Card
<!-- hyz-card-id: 01a1114d-308a-70cc-aa56-74ec99f6c934 -->

### Front

- **增长贡献率**：部分增量占整体增量的比重
- **拉动增长率**：部分增量占整体基期的比重
- 二者有什么关系？

### Back

设部分增量为 $\Delta A$，整体增量为 $\Delta B$，整体基期为 $B_0$：

$$
\boxed{
\text{贡献率}
=
\frac{\Delta A}{\Delta B}
}
$$

$$
\boxed{
\text{拉动增长率}
=
\frac{\Delta A}{B_0}
}
$$

因为：

$$
\frac{\Delta B}{B_0}
=
r_B
$$

所以：

$$
\boxed{
\text{拉动增长率}
=
r_B\times\text{贡献率}
}
$$

另有：

$$
\boxed{
\text{拉动增长率}
=
r_A\times\text{基期比重}
}
$$

记忆：

- **贡献率**看“增量里出了多少力”
- **拉动增长率**看“把整体增速拉了几个百分点”

---

## Card
<!-- hyz-card-id: 22c6efde-a5db-4c8f-9609-ae70c350d902 -->

### Front

贡献率怎么用“现期比 × 增速比”快速估算？

### Back

准确式：

$$
\boxed{
\text{贡献率}
=
\text{基期比重}
\times
\frac{r_A}{r_B}
}
$$

把基期比重换成现期比重：

$$
\boxed{
\text{贡献率}
=
\text{现期比重}
\times
\frac{r_A}{r_B}
\times
\frac{1+r_B}{1+r_A}
}
$$

当部分增速与整体增速差距不大时：

$$
\boxed{
\text{贡献率}
\approx
\text{现期比重}
\times
\text{增速比}
}
$$

误差方向：

- $r_A>r_B$：现期比重 $>$ 基期比重，直接用现期比会**估大**
- $r_A<r_B$：直接用现期比会**估小**

---

## Card
<!-- hyz-card-id: 82b6e6c2-813c-47d4-b13a-a01064602dd2 -->

### Front

名义增长率、实际增长率、价格因素之间是什么关系？如何扣除价格因素？

### Back

核心关系：

$$
\boxed{
1+r_{\text{名义}}
=
(1+r_{\text{实际}})
(1+r_{\text{价格}})
}
$$

所以：

$$
\boxed{
r_{\text{价格}}
=
\frac{r_{\text{名义}}-r_{\text{实际}}}
{1+r_{\text{实际}}}
}
$$

实际现期量：

$$
\boxed{
A_{\text{实际}}
=
\frac{A_{\text{名义}}}
{1+r_{\text{价格}}}
=
A_{\text{名义}}
\times
\frac{1+r_{\text{实际}}}
{1+r_{\text{名义}}}
}
$$

价格因素较小时：

$$
\boxed{
A_{\text{实际}}
\approx
A_{\text{名义}}
(1-r_{\text{价格}})
}
$$

实际增量：

$$
\boxed{
\Delta A_{\text{实际}}
=
\frac{A_{\text{名义}}}
{1+r_{\text{名义}}}
\times
r_{\text{实际}}
}
$$

记忆：

- **名义增长**：钱增长了多少
- **实际增长**：扣除价格变化后，本身增长了多少
- **价格因素**：价格涨了多少

---
