النسخة الإنجليزية: [دليل تقسيم الأقراص](../../docs/partitioning.md) · [الصفحة الرئيسية](../../README.ar.md)

# تقسيم الأقراص لـ distro snapper (`dsnap`)

يشرح هذا الدليل تخطيط Btrfs المتوقع من **distro snapper**، الذي أمره المثبّت
هو `dsnap`.

<a id="easy-installation-path"></a>
## مسار التثبيت السهل

الطريقة الموصى بها للحصول على تخطيط Btrfs الصحيح هي استخدام الأدوات المرافقة
بدلًا من تقسيم الأقراص يدويًا:

- استخدم `dinstaller` لتثبيت distro snapper مع تخطيط الوحدات الفرعية المتوقع؛
- استخدم سكربت [`dlive`](https://github.com/ochibani/dlive) لبناء صورة ISO حية
  تحتوي على `dinstaller` و distro snapper.

لا يزال التقسيم اليدوي مدعومًا، لكن أسهل مسار هو:

```text
ابنِ صورة ISO الحية باستخدام dlive → شغّل الصورة → ثبّت باستخدام dinstaller
```

يتجنّب هذا الأخطاء الشائعة في التخطيط، مثل وضع `/.snapshots` داخل وحدة الجذر
الحيّة أو استخدام تخطيط غير مدعوم لـ `/boot`.

<a id="install-with-dinstaller"></a>
### التثبيت باستخدام dinstaller

يُعد `dinstaller` مسار التثبيت الموصى به لـ distro snapper. ومع إعداد داعم
لـ dsnap، يجب أن ينشئ أو يتحقق من الوحدات الفرعية التالية:

```text
@_linux
@boot_linux
@etc_linux
@var_linux
@.snapshots_linux
```

ويجب أن يضبط أيضًا ما يلي:

```text
/.snapshots
/.snapshots/dsnap/part
/boot
/boot/efi
```

بعد التثبيت، تحقق من النتيجة باستخدام
[قائمة التحقق](#verification-checklist).

<a id="build-a-live-iso-with-dlive"></a>
### بناء صورة ISO حية باستخدام dlive

يمكن لسكربت [`dlive`](https://github.com/ochibani/dlive) بناء صورة ISO حية
تحتوي على `dinstaller` و distro snapper. لم يُأكَّد اسم سكربت البناء بعد؛ إذ
يعيد رابط المستودع حاليًا خطأ 404.

```sh
git clone https://github.com/ochibani/dlive.git
cd dlive
sudo ./<dlive-build-script>.sh
```

يجب أن تحتوي الصورة المولّدة على الأقل على:

- `dinstaller`؛
- `dsnap`؛
- `btrfs-progs`؛
- أدوات GRUB؛
- `efibootmgr` على أنظمة EFI؛
- حزم النواة والبرامج الثابتة اللازمة للجهاز المستهدف.

كتابة صورة ISO إلى قرص خاطئ تُتلف البيانات. تحقق من الجهاز المستهدف قبل
استخدام `dd` أو أي أداة كتابة خام للأقراص.

<a id="scope"></a>
## النطاق

يغطي هذا الدليل التخطيط الذي يستخدمه distro snapper لأنظمة الجذر Btrfs مع
GRUB. لا يُغني هذا الدليل عن توثيق التثبيت الخاص بتوزيعتك.

<a id="hard-requirements"></a>
## المتطلبات الأساسية

- نظام ملفات جذر Btrfs.
- GRUB مع دعم الإقلاع EFI أو BIOS.
- الوحدة الفرعية للجذر `@_linux`.
- الوحدة الفرعية `@boot_linux` مثبّتة عند `/boot`.
- الوحدتان الفرعيتان `@etc_linux` و`@var_linux`.
- الوحدة الفرعية `@.snapshots_linux` مثبّتة عند `/.snapshots`، وخارج `@_linux`.
- المجلد `/.snapshots/dsnap/part` موجود وقابل للكتابة من قِبل الجذر.
- `@srv_linux` كوحدة فرعية مشتركة منفصلة عند `/srv` عند استخدامها.

<a id="recommended-layout"></a>
## التخطيط الموصى به

```text
@_linux                 الجذر
@boot_linux             /boot
@etc_linux              /etc
@var_linux              /var
@.snapshots_linux       /.snapshots
@srv_linux              /srv
```

تُعد مشاركة `/home` و`/srv` وسلوك `/var` الافتراضي أمرًا مقصودًا. لا تُعدّ
مساحات المستخدمين (user namespaces) جزءًا من نموذج الثقة.

<a id="example-initial-fstab"></a>
## مثال لملف fstab أولي

```fstab
UUID=ROOT / btrfs subvol=@_linux,compress=zstd:1 0 0
UUID=ROOT /boot btrfs subvol=@boot_linux,compress=zstd:1 0 0
UUID=ROOT /etc btrfs subvol=@etc_linux,compress=zstd:1 0 0
UUID=ROOT /var btrfs subvol=@var_linux,compress=zstd:1 0 0
UUID=ROOT /.snapshots btrfs subvol=@.snapshots_linux,compress=zstd:1 0 0
UUID=ROOT /srv btrfs subvol=@srv_linux,compress=zstd:1 0 0
UUID=EFI /boot/efi vfat umask=0077 0 2
```

<a id="shared-mutable-directories"></a>
## المجلدات القابلة للتعديل والمشتركة

تُشارك `/home` و`/srv` وبيانات `/var` الافتراضية بين الأشجار. لا تضع حالة
اللقطات أو حالة GRUB أو إعدادات `dsnap` في مجلدات مشتركة قابلة للتعديل.

<a id="verification-checklist"></a>
## قائمة التحقق

```sh
findmnt -T / -o SOURCE,FSTYPE,OPTIONS
findmnt -T /.snapshots -o OPTIONS
btrfs subvolume list /
sudo dsnap tree
sudo dsnap current
sudo dsnap boot-update -s 0
```

<a id="common-mistakes"></a>
## أخطاء شائعة

- وضع `/.snapshots` داخل `@_linux`.
- غياب `@boot_linux` أو تخطيط غير مدعوم لـ `/boot`.
- وضع مخرجات GRUB داخل لقطة ستُستعاد عند التراجع.
- معاملة `/home` أو `/srv` أو `/var` المشتركة على أنها غير قابلة للتغيير.
- تشغيل اختبارات تدميرية على أقراص الإنتاج.
