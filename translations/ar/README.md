# Distro Snapper

## الترجمات

[English](../../README.md) · [العربية](README.md)

**Distro Snapper** أداة سطر أوامر مكتوبة بلغة Rust لإدارة
لقطات Btrfs غير القابلة للتغيير ذات البنية الشجرية على لينكس. يمكن أن
تحتفظ كل لقطة بملف تعريف حزم وتكوين خاصين بها، بالإضافة إلى هدف إقلاع GRUB
خاص بها.

> **مشروع تجريبي.** لا يزال Distro Snapper قيد التطوير النشط. قد تتغير بنى
> اللقطات وسلوك الأوامر وفحوص السلامة وتكامل الحزم دون أي ضمان للتوافق. استخدم
> أقراصًا احتياطية أو نسخًا مؤكدة قبل تشغيل الأوامر التي تعدّل الأقراص أو
> اللقطات أو مداخل الإقلاع، واقرأ
> [دليل تقسيم الأقراص](partitioning.md) أولًا.

## روابط سريعة

- [مسار التثبيت السهل](partitioning.md#easy-installation-path)
- [المتطلبات الأساسية](partitioning.md#hard-requirements)
- [التخطيط الموصى به](partitioning.md#recommended-layout)
- [إعداد أول إقلاع](#first-boot-setup)
- [البناء من المصدر](#build-from-source)
- [بناء الحزم](#build-packages)
- [مرجع الأوامر](commands.md)
- [التاريخ والإسناد](#التاريخ-والإسناد)

<a id="first-boot-setup"></a>
## إعداد أول إقلاع

على نظام حديث التثبيت، هيّئ أول شجرة عمل قبل إجراء أي عمل آخر باللقطات. يترك
المثبّت النشر الأساسي كما هو، ويحتاج `dsnap` إلى إنشاء أول شجرة قابلة للاستخدام
منه وتحديدها كهدف للإقلاع التالي.

نفّذ هذه الأوامر مرة واحدة بصلاحيات الجذر:

```sh
sudo dsnap clone -s 0
sudo dsnap deploy -s 1
sudo reboot
```

يُنشئ أمر clone أول شجرة عمل من اللقطة 0، ويجعل أمر deploy اللقطة 1 هي هدف
الإقلاع التالي، ثم يطبّق إعادة الإقلاع تغييرات GRUB ووحدات التركيب.

نفّذ الأوامر كل على حدة كما هو موضح. لا تستبدل السطر الأول بـ `&`، لأن ذلك
سيُرسل عملية clone إلى الخلفية بدل انتظار اكتمالها.

تجاوز هذه الخطوة فقط إذا كان المثبّت قد أنشأ اللقطة 1 ونشرها بالفعل.

<a id="build-from-source"></a>
## البناء من المصدر

يُبنى Distro Snapper باستخدام Cargo. مجموعة المزايا الافتراضية هي
`base-import,btrfs,grub,import,pacman`.

تختار المزايا `apt` و`dnf` و`pacman` الواجهة الخلفية لمدير الحزم وقت الترجمة.
لا يمكن استخدام أكثر من واحدة منها معًا؛ اختر واحدة فقط للتوزيعة المستهدفة.

قبل البناء، اضبط متغير توافق PyO3:

```sh
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
```

هذا مطلوب لأن المشروع يثبّت `pyo3 0.21.2`، وتتطلب مفسرات بايثون الأحدث وضع
ABI المستقر المتوافق مع الإصدارات الأقدم.

### مزايا Cargo

| الميزة | الغرض |
| --- | --- |
| `base-import` | تفعيل أمر `base-import` لاستعادة اللقطة 0 من تيار مُتحقّق منه. |
| `btrfs` | تفعيل دعم وحدات Btrfs الفرعية واللقطات؛ يجلب `libbtrfs`. |
| `grub` | تفعيل دعم أهداف إقلاع GRUB ومدخل الاستعادة. |
| `import` | تفعيل أمر `import`. |
| `lock` | تفعيل دعم تثبيت الحزم، مثل `apt-mark hold` على الأنظمة المبنية على Debian. |
| `apt` | الواجهة الخلفية لـ APT وقت الترجمة، لبنائي Debian/Ubuntu. |
| `dnf` | الواجهة الخلفية لـ DNF وقت الترجمة، لبناء Fedora. |
| `pacman` | الواجهة الخلفية لـ Pacman وقت الترجمة، لبناء Arch. |

يستخدم البناء العادي مجموعة المزايا الافتراضية:

```sh
cargo build --locked
```

### Arch Linux

ثبّت تبعيات البناء والتشغيل:

```sh
sudo pacman -S --needed base-devel cargo git btrfs-progs curl efibootmgr gnupg python python-anytree grub
```

يمكن للواجهة الخلفية لـ Pacman استخدام `paru` أيضًا لحزم AUR؛ ثبّتها من AUR إذا
كانت ملفات التعريف لديك تحتاج دعم AUR.

ابنِ بوضوح باستخدام واجهة Arch:

```sh
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
cargo build --release --locked --no-default-features \
  --features "base-import,btrfs,grub,import,lock,pacman"
```

### Fedora

ثبّت تبعيات البناء والتشغيل:

```sh
sudo dnf install cargo rust gcc make pkgconf-pkg-config openssl-devel \
  libcurl-devel python3-devel btrfs-progs curl efibootmgr gnupg2 \
  python3 python3-anytree
```

ابنِ بوضوح باستخدام واجهة DNF:

```sh
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
cargo build --release --locked --no-default-features \
  --features "base-import,btrfs,grub,import,lock,dnf"
```

### Debian / Ubuntu

ثبّت تبعيات البناء والتشغيل:

```sh
sudo apt install build-essential cargo rustc pkg-config libssl-dev \
  libcurl4-openssl-dev libpython3-dev file debhelper btrfs-progs curl \
  efibootmgr gnupg python3-anytree
```

ابنِ بوضوح باستخدام واجهة APT:

```sh
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
cargo build --release --locked --no-default-features \
  --features "base-import,btrfs,grub,import,lock,apt"
```

<a id="build-packages"></a>
## بناء الحزم

توجد وصفات التغليف في `packaging/`. تثبّت هذه الوصفات `/usr/bin/dsnap` ووحدات
systemd وصفحات man وإكمالات الصدفة. إضافة الحزمة المبنية إلى مستودع تقع على
عاتق المستخدم أو مدير الحزم.

### Arch Linux

يبني ملف PKGBUILD حزمة `distro-snapper` من وسم إصدار:

```sh
cd packaging
makepkg -si
```

### Fedora

جهّز شجرة بناء RPM وأرشيف المصدر وملفات systemd:

```sh
sudo dnf install rpmdevtools rpm-build
rpmdev-setuptree

git archive --format=tar.gz --prefix=distro-snapper-0.1.0/ \
  -o ~/rpmbuild/SOURCES/distro-snapper-0.1.0.tar.gz HEAD

mkdir -p ~/rpmbuild/SOURCES/packaging/systemd
cp packaging/systemd/dsnap-auto-upgrade.service \
   packaging/systemd/dsnap-auto-upgrade.timer \
   ~/rpmbuild/SOURCES/packaging/systemd/

rpmbuild -ba packaging/rpm/distro-snapper.spec
```

### Debian / Ubuntu

توجد ملفات التغليف في `packaging/debian`. انسخها إلى `debian/` قبل البناء:

```sh
cp -r packaging/debian debian
dpkg-buildpackage -us -uc -b
rm -rf debian
```

تستخدم وصفتا Debian وFedora الميزتين `apt` و`dnf` على التوالي. يستخدم ملف
PKGBUILD الخاص بـ Arch الواجهة الافتراضية Pacman.

## التاريخ والإسناد

لم يبدأ Distro Snapper من الصفر. إنه الاسم الحالي في سلسلة من المشاريع التي
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
   الحزم وسير عمل اللقطات التي تحوّلت لاحقًا إلى Distro Snapper.
4. يُعد [Distro Snapper](https://github.com/ochibani/dsnap)، بأمره `dsnap`،
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
بتغيير في تلك المشاريع. يبقى Distro Snapper مشروعًا تجريبيًا مستقلًا، لكنه
مدين بتصميمه وعمله التنفيذي المبكر لجهودهم.
