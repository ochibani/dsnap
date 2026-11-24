النسخة الإنجليزية: [مرجع الأوامر](../../docs/commands.md) · [الصفحة الرئيسية](README.md)

# مرجع الأوامر لـ distro snapper (`dsnap`)

تفترض كل الأمثلة صلاحيات الجذر عبر `sudo`. تظل صفحة man المولّدة هي المصدر
الدقيق للأعلام، بينما تشرح هذه الصفحة الهدف وتعطي أمثلة بداية آمنة.

الشارات:

- **للقراءة فقط** — لا يعدّل اللقطات؛
- **يُعدّل** — يعدّل نسخة أو عملية نشر؛
- **تدميري** — قد يحذف بيانات أو يغيّر حالة الإقلاع.

## فحص النظام

| الأمر | ماذا يفعل | مثال |
| --- | --- | --- |
| `current` | **للقراءة فقط** يعرض رقم اللقطة الحالية. | `sudo dsnap current` |
| `tree` | **للقراءة فقط** يعرض شجرة اللقطات. | `sudo dsnap tree` |
| `list` | **للقراءة فقط** يعرض الحزم المثبّتة في لقطة؛ الخيار `-e` يستبعد التبعيات. | `sudo dsnap list -s 3 -e` |
| `whichsnap` | **للقراءة فقط** يجد اللقطات التي تحتوي حزمة معيّنة. | `sudo dsnap whichsnap -p firefox` |
| `whichtmp` | **للقراءة فقط** يعرض اللقطة المستخدمة في النشر الحالي. | `sudo dsnap whichtmp` |
| `subvolumes` | **للقراءة فقط** يعرض وحدات Btrfs الفرعية. | `sudo dsnap subvolumes` |
| `last-update` | **للقراءة فقط** يتحقق من حالة آخر تحديث. | `sudo dsnap last-update` |
| `version` | **للقراءة فقط** يعرض إصدار dsnap. | `sudo dsnap version` |
| `difference` | **للقراءة فقط** يعرض الفرق في الحزم بين لقطتين. | `sudo dsnap difference -s 3 -t 4` |
| `description` | **يُعدّل** يضبط أو يستبدل وصف لقطة. | `sudo dsnap description -s 3 -d "KDE test"` |

## إنشاء الأشجار واستنساخها وحذفها

| الأمر | ماذا يفعل | مثال |
| --- | --- | --- |
| `new-tree` | **يُعدّل** ينشئ شجرة من المستوى الأعلى من اللقطة 0. | `sudo dsnap new-tree -d "GNOME workstation"` |
| `add-branch` | **يُعدّل** ينشئ لقطة فرعية. | `sudo dsnap add-branch -s 3 -d "try NVIDIA driver"` |
| `clone` | **يُعدّل** ينشئ شجرة من المستوى الأعلى من نسخة لقطة. | `sudo dsnap clone -s 3 -d "backup copy"` |
| `clone-branch` | **يُعدّل** ينشئ لقطة شقيقة. | `sudo dsnap clone-branch -s 3` |
| `clone-tree` | **يُعدّل** يستنسخ لقطة وكل أحفادها بشكل متكرر. | `sudo dsnap clone-tree -s 3` |
| `clone-under` | **يُعدّل** ينسخ لقطة إلى فرع تحت أبٍ آخر. | `sudo dsnap clone-under -s 2 -b 7` |
| `rebuild` | **يُعدّل** يعيد بناء شجرة جديدة من ملف تعريف محفوظ. | `sudo dsnap rebuild -s 3 -d "fresh rebuild"` |
| `delete` | **تدميري** يحذف لقطة وكل أحفادها. | `sudo dsnap delete -s 7` |
| `reset` | **تدميري** يحذف كل اللقطات غير الأساسية، وينشر اللقطة 0، ثم يعيد التشغيل. | `sudo dsnap reset` |

## الحزم وملفات التعريف والخدمات

| الأمر | ماذا يفعل | مثال |
| --- | --- | --- |
| `install` | **يُعدّل** يثبّت الحزم أو ملفات التعريف. | `sudo dsnap install -s 3 -p vim git` |
| `install` profile | **يُعدّل** يثبّت ملف تعريف مجمّعًا. | `sudo dsnap install -s 3 -P desktop-gnome` |
| `uninstall` | **يُعدّل** يزيل الحزم أو ملفات التعريف. | `sudo dsnap uninstall -s 3 -p vim` |
| `upgrade` | **يُعدّل** يحدّث الحزم في لقطة. الخيار `-l` يحدّث النشر الحي في مكانه. | `sudo dsnap upgrade -s 3 -y` |
| `auto-upgrade` | **يُعدّل** مسار التحديث غير التفاعلي. | `sudo dsnap auto-upgrade -s 3` |
| `refresh` | **يُعدّل** يحدّث بيانات مستودعات الحزم. | `sudo dsnap refresh -s 3` |
| `fixdb` | **يُعدّل** يعيد بناء حلقة المفاتيح وقاعدة بيانات الحزم. | `sudo dsnap fixdb -s 3` |
| `edit-profile` | **يُعدّل** يعدّل ملف تعريف لقطة. | `sudo dsnap edit-profile -s 3` |
| `edit-config` | **يُعدّل** يعدّل ملف `dsnap.conf`. | `sudo dsnap edit-config -s 3` |
| `service` | **يُعدّل** يفعّل أو يعطّل وحدات systemd. | `sudo dsnap service -s 3 -e sshd.service` |
| `tree-install` | **يُعدّل** يثبّت عبر شجرة كاملة. | `sudo dsnap tree-install -t 3 -P desktop-gnome` |
| `tree-uninstall` | **يُعدّل** يزيل عبر شجرة كاملة. | `sudo dsnap tree-uninstall -t 3 -p vim` |
| `tree-upgrade` | **يُعدّل** يحدّث شجرة كاملة. | `sudo dsnap tree-upgrade -t 3` |
| `tree-sync` | **يُعدّل** يحدّث ويزامن شجرة كاملة بشكل متكرر؛ يتطلب اتصالًا بالإنترنت. | `sudo dsnap tree-sync -t 3` |

## النشر والاستعادة وبيئات chroot

| الأمر | ماذا يفعل | مثال |
| --- | --- | --- |
| `deploy` | **يُعدّل** يضبط هدف الإقلاع الافتراضي للإقلاع التالي. الخيار `-S` يستخدم الفتحة الثانوية. | `sudo dsnap deploy -s 3` |
| `boot-update` | **يُعدّل** يعيد توليد إعدادات GRUB للقطة. | `sudo dsnap boot-update -s 3` |
| `efi-update` | **يُعدّل** يبدّل مدخل إقلاع EFI بشكل تفاعلي. | `sudo dsnap efi-update` |
| `etc-update` | **يُعدّل** يحفظ تغييرات `/etc` الحية في اللقطة الحالية. | `sudo dsnap etc-update` |
| `rollback` | **تدميري** ينسخ النشر السابق ويجعله هدف الإقلاع التالي. | `sudo dsnap rollback` |
| `hollow` | **يُعدّل** يثبّت لقطة قابلة للكتابة عند `/`. | `sudo dsnap hollow -s 3` |
| `chroot` | **يُعدّل** يفتح صدفة جذر معزولة داخل نسخة قابلة للكتابة. | `sudo dsnap chroot -s 3` |
| `live-chroot` | **تدميري** صدفة استعادة داخل اللقطة المُقلَعة؛ تُهمَل تغييراتها عند النشر التالي. | `sudo dsnap live-chroot` |
| `run` | **يُعدّل** ينفّذ أمرًا داخل نسخة قابلة للكتابة ويحفظ النتيجة. | `sudo dsnap run -s 3 -c "vim /etc/fstab"` |
| `tree-run` | **يُعدّل** ينفّذ أمرًا في نسخ قابلة للكتابة عبر شجرة كاملة. | `sudo dsnap tree-run -t 3 -c "update-ca-certificates"` |
| `unlock` | **تدميري** يزيل قفل chroot ويهمل التغييرات غير المثبّتة. | `sudo dsnap unlock -s 3` |
| `clean-tmp` | **تدميري** ينظّف لقطات العمل غير المثبّتة لـ chroot. | `sudo dsnap clean-tmp` |
| `immutable-enable` | **يُعدّل** يفعّل عدم قابلية اللقطة للتغيير. | `sudo dsnap immutable-enable -s 3` |
| `immutable-disable` | **يُعدّل** يعطّل عدم قابلية اللقطة للتغيير. | `sudo dsnap immutable-disable -s 3` |
| `base-update` | **يُعدّل** يحدّث اللقطة 0 ونشر الاستعادة. | `sudo dsnap base-update -y` |
| `base-rebuild` | **تدميري** يعيد بناء اللقطة 0 من حزم الإقلاع الأولي. | `sudo dsnap base-rebuild` |

## الاستيراد والتصدير

| الأمر | ماذا يفعل | مثال |
| --- | --- | --- |
| `export` | **يُعدّل** يصدّر تيار Btrfs مضغوطًا وموقّعًا مع ملف SHA-512 مرافق. | `sudo dsnap export -s 3 -o /srv/dsnap-exports -k ABC123` |
| `import` | **يُعدّل** يستورد تيارًا مُتحقّقًا منه كلقطة جديدة. | `sudo dsnap import -p /srv/dsnap-exports/stream -d "restored tree"` |
| `base-import` | **تدميري** يستبدل اللقطة 0 من تيار مُتحقّق منه. | `sudo dsnap base-import -p /srv/dsnap-exports/base-stream` |

مسارات الاستيراد في الأمثلة توضيحية فقط. احتفظ بالملفين المرافقين
`.sha512` و`.sig` بجانب التيار، وتجنّب `--allow-unverified` إلا للبيانات
الموثوقة والمُراجَعة.
