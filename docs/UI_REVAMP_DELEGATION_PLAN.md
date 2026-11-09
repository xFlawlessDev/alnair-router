# Rencana delegasi revamp UI Alnair

Status: dieksekusi dengan tiga agen `gpt-6.1-terra` dan satu koordinator. Hasil integrasi dan cakupan pemeriksaan dicatat di [UI_REVAMP_REPORT.md](UI_REVAMP_REPORT.md).

## Tujuan dan cakupan

Revamp seluruh 16 halaman `apps/web/src/pages` menjadi editorial magazine dengan kolase dan stiker retro, mengikuti tiga gambar referensi pengguna. Hasilnya harus tetap nyaman untuk pekerjaan administrasi, pembacaan data, dan penggunaan melalui keyboard maupun ponsel.

Rencana ini memakai empat slot: satu koordinator dan maksimal tiga agen pelaksana aktif. Agen baru dijalankan ketika implementasi dimulai. Semua agen membaca `AGENTS.md`, `apps/web/AGENTS.md`, kontrak desain dari tahap 1, serta skill Vue dan antislop yang relevan. Mode antislop sesi ini adalah `during`.

Perubahan meliputi tema, tipografi, komposisi, responsivitas, dan presentasi komponen fitur. Navigasi yang ada, identitas logo, kontrak API, autentikasi, dan perilaku bisnis tetap menjadi dasar implementasi. Temuan bug UX dari audit sebelumnya dicatat sebagai pekerjaan tersendiri; jangan menyisipkan perubahan logika yang tidak diperlukan untuk revamp.

## Arah visual bersama

Design Read: tiga referensi menunjukkan kertas krem bertekstur, bidang navy dan kobalt, tepi sobek, halftone, potongan foto berbingkai putih, cap cetak, serta anotasi kuning dan merah. Judul dominan berupa sans-serif tebal dan sedikit miring. Foto jembatan dan figur manusia adalah contoh teknik kolase, bukan aset produk yang perlu disalin.

Dials awal, sebagai arahan desain: ENERGY 3/5, RHYTHM 3/5, MOTION 1/5. Energi datang dari komposisi dan aksen cetak; gerak dibatasi pada feedback interaksi. Isi tabel dan formulir tetap lurus.

| Unsur       | Keputusan dan tujuan                                                                                                                                          |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Palet utama | Krem kertas, navy tinta, dan kobalt untuk identitas serta hierarki. Nilai warna final ditetapkan setelah uji kontras.                                         |
| Aksen       | Mustard untuk sorotan; vermilion untuk anotasi atau keadaan yang perlu perhatian. Warna semantik error, warning, dan success tetap dibedakan lewat teks/ikon. |
| Tipografi   | Display sans tebal untuk judul; sans yang mudah dibaca untuk isi; monospace untuk model, key, endpoint, dan log. Heading miring bersifat selektif.            |
| Motif       | Garis cetak, tepi kertas sobek pada bagian utama, dan stiker berbingkai putih sebagai pola identitas berulang.                                                |
| Permukaan   | Tekstur hanya pada latar atau bingkai. Tabel, field, popover, dialog, dan grafik mendapat permukaan bersih.                                                   |
| Stiker      | Maksimal satu atau dua aksen dekoratif per layar. Label operasional tidak diputar; dekorasi tidak menghalangi klik atau focus ring.                           |
| Layout      | Judul rata kiri; ringkasan punya fokus utama; variasi komposisi mengikuti isi halaman.                                                                        |
| Tema gelap  | Navy menjadi permukaan utama dengan teks terang dan kobalt yang disesuaikan. Tema yang dipilih pengguna tetap dihormati.                                      |
| Gerak       | Transisi singkat; reduced motion mencakup indikator live dan animasi topologi yang sudah ada.                                                                 |

Logo yang ada dipakai kembali. Dekorasi kertas, halftone, dan bingkai sederhana dapat dibuat langsung dalam CSS/SVG sebagai bagian implementasi. Ilustrasi raster, maskot, logo baru, dan foto baru tidak termasuk pekerjaan awal. Bila dibutuhkan, ajukan brief aset terpisah sebelum membuatnya; jangan menjadikan kebutuhan aset tambahan sebagai penghambat pekerjaan UI yang sudah jelas.

## Tahap 1: fondasi dan halaman percontohan

Pemilik: koordinator. Tahap ini selesai sebelum tiga agen mengubah halaman.

1. Periksa keadaan repo terbaru dan ketentuan lokal. Pastikan screenshot referensi dapat dibuka; jangan memasukkan screenshot pribadi ke bundle produksi.
2. Tulis `apps/web/DESIGN.md` sebagai kontrak visual: token warna kedua tema, font dan fallback, ukuran teks, spacing, radius, border, shadow, serta contoh penggunaan stiker.
3. Perbarui `apps/web/src/styles.css` menggunakan semantic tokens yang sudah ada. Tambahkan token editorial seperlunya; hindari warna lokal yang berbeda antarhalaman.
4. Terapkan fondasi ke `components/layout/`, `PageHeader.vue`, `EmptyState.vue`, `StatusBadge.vue`, dan `ConfirmDialog.vue`. Pertahankan API primitive dalam `components/ui/`; utamakan token, class aplikasi, atau wrapper. Jangan mengedit primitive secara massal.
5. Jika dibutuhkan, buat komponen presentasi di `components/editorial/`. Setiap komponen harus punya satu tanggung jawab dan kontrak props/slots yang terdokumentasi. Misalnya bingkai header, label stiker, atau pemisah kertas; jangan membangun abstraksi sebelum ada penggunaan nyata.
6. Terapkan contoh lengkap di `OverviewPage.vue`, termasuk ringkasan, peringatan, dan katalog. Gunakan data runtime yang ada; fixture hanya untuk validasi lokal.
7. Periksa contoh di kedua tema, desktop dan ponsel. Bekukan kontrak komponen serta daftar file bersama sebelum fan-out. Screenshot dan keputusan desain menjadi acuan semua agen.

Kriteria keluar: fondasi dapat dibangun, pola komponen dapat digunakan halaman lain, kontras terukur, dan contoh Overview tidak memiliki overflow atau dekorasi yang menutup data. Review koordinator menjadi gerbang teknis; umpan balik pengguna dapat mengarahkan revisi tanpa menambah permintaan izin rutin.

## Tahap 2: implementasi paralel dengan kepemilikan file

Koordinator tetap memiliki `styles.css`, `components/layout/`, komponen bersama di akar `components/`, `components/editorial/`, `DESIGN.md`, serta konfigurasi dan dependensi. Kebutuhan perubahan pada file tersebut dikirim sebagai permintaan kepada koordinator.

| Agen                      | Halaman yang dimiliki                                                            | Direktori komponen fitur yang dimiliki                     | Fokus                                                                                                       |
| ------------------------- | -------------------------------------------------------------------------------- | ---------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| A: pengelolaan            | Connections, Aliases, Combos, Keys, Pricing                                      | `connections/`, `aliases/`, `combos/`, `keys/`, `pricing/` | Direktori editorial yang dapat dipindai, aksi jelas, tabel responsif, formulir dan dialog konsisten.        |
| B: pemantauan             | Overview, Usage, Console                                                         | `usage/`                                                   | Hierarki statistik, keterbacaan log, chart dan topology dalam kedua tema, presentasi data pada layar kecil. |
| C: konfigurasi dan konten | Settings, TokenSaver, Playground, PublicUsage, Guide, Changelog, Login, NotFound | `settings/`, `token-saver/`, `playground/`                 | Formulir, tab, halaman publik, panduan, dan release notes menggunakan bahasa visual yang sama.              |

Nama halaman pada tabel merujuk ke file `*Page.vue` di `src/pages/`. Tes yang berada di samping file mengikuti pemilik fitur yang sama. `UsageSummaryCards.vue` adalah file bersama milik koordinator; `components/usage/` milik B juga dipakai PublicUsage milik C. C mengirim kebutuhan perubahan komponen usage ke B, bukan mengeditnya langsung.

Overview berpindah dari koordinator ke B setelah tahap 1 dinyatakan selesai. C mengerjakan dua batch: pertama Settings, TokenSaver, Playground, PublicUsage; kemudian Guide, Changelog, Login, NotFound. Jika C tertinggal, pindahkan batch kedua ke agen yang selesai melalui handoff eksplisit sebelum file mulai diedit.

### Target per halaman

| Halaman     | Hasil visual yang dituju                                                                                 |
| ----------- | -------------------------------------------------------------------------------------------------------- |
| Overview    | Halaman depan editorial: judul kuat, satu fokus statistik, status jelas, katalog tetap terbaca.          |
| Connections | Kartu seperti entri direktori provider; mode tabel dan toolbar selaras dengan tema.                      |
| Aliases     | Prefix dan tujuan koneksi mudah dipasangkan; aksi tidak mendominasi data.                                |
| Combos      | Urutan tier terbaca sebagai alur vertikal, dengan penanda urutan bergaya cetak.                          |
| Keys        | Nama, status, dan batas utama terlihat lebih dahulu; detail panjang dapat dibuka tanpa memenuhi layar.   |
| Pricing     | Kolom harga menyebut unit, angka sejajar, override mudah dikenali lewat label dan tampilan.              |
| Usage       | Ringkasan dan data request punya hierarki jelas; topologi dan grafik tidak menghambat akses ke tabel.    |
| Console     | Tipografi log terbaca, level dan waktu dibedakan, kontras aman di kedua tema.                            |
| Settings    | Rubrik konfigurasi jelas; field, bantuan, validasi, dan bar simpan konsisten.                            |
| TokenSaver  | Savings dan Configuration berbagi tema; measured dan estimated dapat dibedakan tanpa mengandalkan warna. |
| Playground  | Area input/output fokus; kontrol model dan saver tetap mudah ditemukan; streaming dan error terbaca.     |
| PublicUsage | Ringkasan personal sederhana, tabel model responsif, kolom key dan aksi koneksi jelas.                   |
| Guide       | Susunan rubrik majalah, kode dengan permukaan bersih dan scroll lokal, hierarki bacaan jelas.            |
| Changelog   | Versi menjadi heading editorial; kategori dan status update menjadi label sekunder.                      |
| Login       | Komposisi ringkas dengan satu aksen retro; label dan error formulir paling mudah dipahami.               |
| NotFound    | Pesan singkat dengan karakter editorial dan jalur kembali yang terlihat jelas.                           |

## Kontrak kerja antaragen

- Semua agen menggunakan semantic tokens serta komponen dari fondasi. Jangan membuat palet, font, tombol, atau pola stiker sendiri.
- Perubahan file hanya dalam kepemilikan masing-masing. Sebelum memindahkan file atau tanggung jawab, koordinator menetapkan pemilik baru dan memberi tahu pemilik lama.
- Bila berbagi working directory, jangan melakukan `git reset`, checkout massal, reformat seluruh repo, atau menyelesaikan konflik dengan membuang perubahan agen lain. Koordinator meninjau diff akhir sebagai satu hasil terintegrasi.
- Gunakan Composition API, `<script setup lang="ts">`, props/emits bertipe, dan komponen fitur kecil. Pisahkan presentasi bila halaman mulai memegang banyak tanggung jawab.
- Route view tetap menjadi tempat komposisi fitur. Ekstraksi file tetap berada di direktori fitur milik agen terkait.
- Request data dan mutasi tetap melalui `src/lib/api.ts`; feedback mutasi memakai toast dan tindakan destruktif memakai ConfirmDialog.
- Perubahan pada `src/lib/`, router, types API, backend, package manifest, atau lockfile disampaikan kepada koordinator terlebih dahulu untuk penetapan pemilik dan penilaian scope. Jangan menambah dependensi hanya demi efek dekoratif.
- Screenshot validasi memakai data sintetis yang dilabeli sebagai fixture dan key palsu. Jangan merekam secret atau memasukkan klaim statistik rekaan ke UI produksi.
- Setiap agen melaporkan perubahan, pemeriksaan yang benar-benar dijalankan, bukti visual, dan kekurangan tersisa. Jangan menyatakan PASS untuk pemeriksaan yang belum dilakukan.

## Tahap 3: integrasi dan verifikasi

Pemilik: koordinator, setelah seluruh handoff diterima.

### Matriks visual dan interaksi

- Semua 16 halaman: lebar 375, 768, dan 1440 px, masing-masing dalam tema terang dan gelap. Tambahkan 320 px untuk pemeriksaan overflow dasar dan uji zoom teks 200% pada alur utama.
- Untuk halaman yang relevan, periksa initial loading, data terisi, kosong, error, teks/model sangat panjang, serta dialog atau sheet terbuka. Halaman statis diberi N/A pada state yang tidak relevan.
- Tidak ada overflow pada dokumen. Tabel/kode boleh memiliki scroll lokal yang jelas; untuk tabel utama di ponsel, prioritaskan kolom atau tampilan ringkas dengan akses ke detail lengkap.
- Uji keyboard: urutan Tab, focus ring, Enter/Space, Escape, dialog focus trap, dan pengembalian fokus setelah menutup overlay.
- Ukur kontras dari style hasil render: minimal 4.5:1 untuk teks biasa, 3:1 untuk teks besar serta batas/indikator kontrol yang relevan. Periksa warna status dan Console secara khusus.
- Area sentuh kontrol utama sekitar 44 x 44 px; ikon boleh lebih kecil secara visual. Jangan menyelesaikan kepadatan tabel dengan mengecilkan semua teks dan target.
- Verifikasi reduced motion, scroll panjang, bar simpan, menu ponsel, serta header tidak menutup konten.
- Klik seluruh kontrol yang relevan dan catat hasilnya: navigasi, tema, refresh, filter, pagination, tab, buka/tutup dialog, salin, reveal/hide, dan submit. Uji tindakan destruktif atau mutasi hanya pada lingkungan lokal dengan data fixture yang dapat dibuang.

### Pemeriksaan otomatis dan bukti

1. Setiap agen menjalankan tes fitur yang relevan setelah mengubah interaksi. Jangan menambah tes yang hanya mengunci class dekoratif.
2. Koordinator menjalankan `pnpm test` dan `pnpm run check` dari `apps/web` setelah integrasi. Build ini juga memperbarui dashboard yang dipakai router.
3. Sesuai instruksi root, jalankan `cargo fmt --all` lalu `cargo clippy --workspace --all-targets -- -D warnings` sebelum menutup implementasi. Jika source Rust berubah, jalankan tes workspace yang sesuai; kegagalan lingkungan atau baseline dilaporkan secara spesifik.
4. Siapkan screenshot per route, tema, dan viewport beserta catatan click-through. Periksa dukungan browser automation yang tersedia saat pelaksanaan; jika belum ada, pilih cara pengujian yang dapat dijalankan dan laporkan cakupannya tanpa mengarang hasil.
5. Jalankan Delivery Gate antislop: Hard Gate, Purpose-Gate, Liveliness, dan Craftsmanship/Quality Locks. Catat alasan teknik visual, hasil nyata, dan item yang belum terverifikasi. Selesaikan kegagalan dalam cakupan sebelum menyatakan UI siap.
6. Periksa `git diff` untuk perubahan di luar scope, aset yang tidak digunakan, dependensi baru, dan file hasil build yang tidak semestinya dikomit. Jangan commit atau publish kecuali diminta.

## Prompt delegasi siap pakai

### Agen A

> Terapkan revamp editorial magazine dan stiker retro sesuai `apps/web/DESIGN.md` dan `docs/UI_REVAMP_DELEGATION_PLAN.md`. Anda memiliki ConnectionsPage, AliasesPage, CombosPage, KeysPage, PricingPage serta komponen fitur dan tes terkait dalam daftar kepemilikan. Gunakan kontrak fondasi, perbaiki presentasi responsif dan dialog, serta pertahankan kontrak data dan perilaku bisnis. Jangan mengedit file bersama. Laporkan perubahan lintas kepemilikan kepada koordinator. Serahkan diff, validasi yang dijalankan, bukti visual yang tersedia, dan kekurangan tersisa.

### Agen B

> Terapkan revamp sesuai `apps/web/DESIGN.md` dan rencana delegasi. Anda memiliki OverviewPage setelah handoff fondasi, UsagePage, ConsolePage, serta `components/usage/` dan tes terkait. Fokus pada hierarki data, tabel responsif, chart/topology, kontras Console, dan reduced motion. Komponen usage juga dipakai PublicUsage: koordinasikan perubahan dengan agen C dan pertahankan API props/emits bila memungkinkan. Jangan mengedit file bersama atau logika polling tanpa penetapan scope dari koordinator. Serahkan diff, hasil validasi nyata, bukti visual, dan kekurangan tersisa.

### Agen C

> Terapkan revamp sesuai `apps/web/DESIGN.md` dan rencana delegasi. Anda memiliki SettingsPage, TokenSaverPage, PlaygroundPage, PublicUsagePage, GuidePage, ChangelogPage, LoginPage, NotFoundPage serta komponen fitur dan tes terkait dalam daftar kepemilikan. Kerjakan dua batch sesuai rencana. Fokus pada form, tab, halaman publik, hierarki editorial konten, dan kedua tema. Gunakan komponen usage milik B; mintalah perubahan melalui B. Jangan mengedit file bersama atau controller settings tanpa penetapan scope dari koordinator. Serahkan diff, hasil validasi nyata, bukti visual, dan kekurangan tersisa.

## Hasil akhir yang diserahkan

- Kontrak desain final di `apps/web/DESIGN.md`.
- Implementasi konsisten pada 16 halaman dan komponen turunannya.
- Bukti visual sebelum/sesudah pada halaman percontohan dan screenshot matriks akhir.
- Catatan click-through, hasil build/test, serta laporan Delivery Gate dengan cakupan yang jujur.
- Daftar tindak lanjut bug UX yang terpisah dari keputusan estetika, bila masih ada.

Dokumen rencana ini selesai ketika pembagian kerja, dependensi, dan kriteria penerimaan dapat dipakai untuk memulai pelaksanaan. Kelengkapan dokumen bukan bukti bahwa revamp sudah dibangun atau diverifikasi.
