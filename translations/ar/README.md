# distro snapper

**distro snapper** أداة سطر أوامر مكتوبة بلغة Rust لإدارة
لقطات Btrfs غير القابلة للتغيير ذات البنية الشجرية على لينكس. يمكن أن
تحتفظ كل لقطة بملف تعريف حزم وتكوين خاصين بها، بالإضافة إلى هدف إقلاع GRUB
خاص بها.

> **مشروع تجريبي.** لا يزال distro snapper قيد التطوير النشط. قد تتغير بنى
> اللقطات وسلوك الأوامر وفحوص السلامة وتكامل الحزم دون أي ضمان للتوافق. استخدم
> أقراصًا احتياطية أو نسخًا مؤكدة قبل تشغيل الأوامر التي تعدّل الأقراص أو
> اللقطات أو مداخل الإقلاع، واقرأ
> [دليل تقسيم الأقراص](partitioning.md) أولًا.

## الترجمات

[English](../../README.md) · [العربية](README.md)

## روابط سريعة

- [مسار التثبيت السهل](partitioning.md#easy-installation-path)
- [المتطلبات الأساسية](partitioning.md#hard-requirements)
- [التخطيط الموصى به](partitioning.md#recommended-layout)
- [مرجع الأوامر](commands.md)
- [التاريخ والإسناد](#التاريخ-والإسناد)

## التاريخ والإسناد

لم يبدأ distro snapper من الصفر. إنه الاسم الحالي في سلسلة من المشاريع التي
طوّرت نموذج لقطات Btrfs غير القابلة للتغيير ذات البنية الشجرية.

مسار التطور كما يلي:

1. [astOS](https://github.com/lambdanil/astOS)، الذي بدأه
   [@lambdanil](https://github.com/lambdanil)، قدّم نهج لقطات أرش لينكس غير
   القابل للتغيير.
2. حمل [AshOS](https://github.com/ashos/ashos) هذا التصميم قُدمًا بوصفه توزيعًا
   ميتا ذا بنية شجرية عالمية، مع أعمال أساسية لـ
   [@i2](https://github.com/i2) و[@lambdanil](https://github.com/lambdanil)
   ومساهمين آخرين.
3. كان [فرع `rust` من ochibani/ash](https://github.com/ochibani/ash/tree/rust)
   السلف التقني المباشر لهذا المستودع. استكشف تنفيذ لغة Rust وواجهات مديري
   الحزم وسير عمل اللقطات التي تحوّلت لاحقًا إلى distro snapper.
4. يُعد [distro snapper](https://github.com/ochibani/dsnap)، بأمره `dsnap`،
   الامتداد الحالي لهذا العمل بوصفه أداة سطر أوامر تعمل بصلاحيات الجذر فقط.

يُوجّه الشكر إلى المطورين والمساهمين السابقين الذين جعلوا ذلك ممكنًا:

- **astOS:** [@lambdanil](https://github.com/lambdanil)،
  [@MenacingPerson](https://github.com/MenacingPerson)،
  [@i2](https://github.com/i2)، [@Elecleus](https://github.com/Elecleus)،
  [@Rtizie](https://github.com/Rtizie) و
  [@ShalokShalom](https://github.com/ShalokShalom).
- **AshOS / ash:** [@i2](https://github.com/i2)،
  [@lambdanil](https://github.com/lambdanil),
  [@vireakvibol](https://github.com/vireakvibol)،
  [@duhdugg](https://github.com/duhdugg)،
  [@xaqbr](https://github.com/xaqbr)،
  [@cybertelx](https://github.com/cybertelx) و
  [@RashiqAzhan](https://github.com/RashiqAzhan).

كما نشكر كل من أبلغ عن مشكلة، أو اختبر اللقطات، أو كتب توثيقًا، أو ساهم
بتغيير في تلك المشاريع. يبقى distro snapper مشروعًا تجريبيًا مستقلًا، لكنه
مدين بتصميمه وعمله التنفيذي المبكر لجهودهم.
