# Revamp UI editorial retro

Implementasi: 10 November 2026. Dilaksanakan oleh koordinator dengan tiga agen `gpt-6.1-terra`, mengikuti [rencana delegasi](UI_REVAMP_DELEGATION_PLAN.md) dan [kontrak desain](../apps/web/DESIGN.md).

## Hasil

Seluruh 16 route view menggunakan arah visual krem kertas, navy tinta, dan kobalt. Header editorial memakai judul tebal miring, halftone terbatas, tepi kertas sobek, dan bidang aksi berbingkai kertas. Overview menyorot kesehatan router dengan stiker mustard. Navigasi aktif memakai mustard; panel data dan form tetap bersih.

- Aliases dan Combos menjadi direktori responsif bernomor. Tier combo disusun sebagai urutan vertikal dengan seluruh aksi tetap tersedia.
- Connections memakai toolbar yang membungkus dan kartu direktori; tampilan tabel tetap tersedia.
- Keys mendapat ringkasan khusus ponsel serta tabel lengkap untuk rules, usage, dan actions. Pricing memberi label unit USD per million dan meratakan angka harga.
- Usage menempatkan ringkasan sebelum topologi dan memberi heading khusus request ledger. Tabel lebar menggunakan scroll lokal.
- Console memakai warna semantik kedua tema dan metadata yang lebih terbaca.
- Settings dan Token Saving memakai panel konfigurasi dan bar simpan konsisten. Grid Settings diperbaiki setelah screenshot menemukan pemotongan konten di ponsel.
- Playground, My Usage, Guide, Changelog, Login, dan Not Found mengikuti tipografi dan permukaan yang sama. My Usage terhubung mendapat heading halaman yang eksplisit.
- Ditambahkan landmark main, skip link, penanda navigasi aktif, area sentuh yang diperluas pada pointer sentuh, dan reduced motion. Warna label sumbu Unovis diselaraskan ke token tema setelah pemeriksaan grafik berisi data.

Kontrak API, primitive UI, dependensi, backend, dan logo tidak diubah. Tidak ada publikasi atau commit otomatis. Temuan bug UX lama seperti perlindungan draft settings dan race respons polling tetap merupakan tindak lanjut terpisah.

## Validasi

| Pemeriksaan                                             | Hasil                                                                                                                                                                                                                                                                                                 |
| ------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pnpm test --maxWorkers=1`                              | 33 file, 253 tes lulus.                                                                                                                                                                                                                                                                               |
| `pnpm run check`                                        | Type checking dan production build lulus.                                                                                                                                                                                                                                                             |
| `pnpm run build` setelah penyelarasan panel terakhir    | Lulus; dist lokal diperbarui.                                                                                                                                                                                                                                                                         |
| `cargo fmt --all`                                       | Lulus; tidak ada perubahan source Rust.                                                                                                                                                                                                                                                               |
| `cargo clippy --workspace --all-targets -- -D warnings` | Lulus.                                                                                                                                                                                                                                                                                                |
| `git diff --check`                                      | Lulus pada diff integrasi.                                                                                                                                                                                                                                                                            |
| Browser Chromium                                        | 16 route, lebar 375/768/1440, tema terang dan gelap. Pemeriksaan otomatis meliputi overflow dokumen dan runtime error; screenshot juga ditinjau melalui contoh penuh dan contact sheet.                                                                                                               |
| Pemeriksaan layout tambahan                             | Seluruh route pada 320px; enam halaman utama pada zoom CSS 200%; pemeriksaan lebar konten dalam pada tujuh route di empat viewport. Tidak ada overflow yang terdeteksi setelah koreksi Settings.                                                                                                      |
| Smoke interaksi                                         | 15 skenario lulus: tema, skip link, lima dialog create beserta fokus dan Escape, cancel delete alias, settings/save bar, tab token saver, tab playground, Console, Pricing, My Usage reconnect, dan navigasi ponsel.                                                                                  |
| State                                                   | 12 route data diperiksa pada respons kosong, error, dan loading; tidak ada runtime error atau overflow dokumen. Array fixture dikosongkan untuk empty state, bukan simulasi database tanpa konfigurasi secara menyeluruh.                                                                             |
| Kontras                                                 | Sampel teks HTML yang terlihat pada 16 route dalam dua tema tidak menemukan rasio di bawah 4.5:1 untuk teks biasa atau 3:1 untuk teks besar setelah transisi tema selesai. Teks tersembunyi dan kontrol disabled dikecualikan. Grafik diperiksa terpisah dan label sumbu diselaraskan ke token muted. |
| Grafik berisi data                                      | Usage trend pada 375/1440px, kedua tema: tidak ada overflow atau runtime error; warna label sumbu mengikuti tema.                                                                                                                                                                                     |
| Reduced motion                                          | Durasi animasi umum turun menjadi 0.01ms; animasi edge Vue Flow dinonaktifkan dalam mode reduce.                                                                                                                                                                                                      |

Run tes awal dengan proses build/browser lain berjalan bersamaan mengalami timeout pagination Usage. Run terpisah dengan satu worker lulus tanpa mengubah batas timeout. Suite masih mengeluarkan pesan jsdom `window.scrollTo` yang belum diimplementasikan; tidak menyebabkan kegagalan tes. Build mengeluarkan peringatan ukuran chunk utama sekitar 1.08MB sebelum gzip; pemisahan bundle tidak termasuk perubahan visual ini.

Pemeriksaan browser memakai fixture lokal yang ditandai sebagai data sintetis. Tidak ada key asli di screenshot, tidak ada permintaan completion ke provider nyata, dan tidak ada mutasi pada data router pengguna. Hasil ini merupakan pemeriksaan integrasi presentasi dan smoke test, bukan sertifikasi aksesibilitas menyeluruh atau pengujian setiap kombinasi data/provider.

## Delivery Gate antislop

| Blok                            | Hasil dalam cakupan revamp                                                                                                                                                     |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Hard Gate                       | Lulus pada cakupan browser, keyboard, state, dan build di atas. Dekorasi tidak menerima pointer; data angka berasal dari API runtime, fixture hanya digunakan dalam pengujian. |
| Purpose-Gate                    | Lulus. Warna, tipografi, tekstur, tepi sobek, stiker status, glyph aksi, dan offset shadow memiliki tujuan tertulis dalam DESIGN.md.                                           |
| Liveliness                      | Lulus. ENERGY 3 / RHYTHM 3 / MOTION 1 pada skala tiga tingkat. Header, direktori, form, log, dan konten panduan memiliki komposisi sesuai isi.                                 |
| Craftsmanship dan Quality Locks | Lulus untuk integrasi yang diperiksa: kedua tema, responsivitas, semantic tokens, keadaan runtime, tes dan build. Tidak ada aset atau klaim produk rekaan yang ditambahkan.    |

## Bukti lokal

Bukti browser berada di `.dev-home/ui-review/`, direktori kerja yang diabaikan Git. Script pemeriksaan juga berada di `.dev-home/`, memakai Playwright dan Chromium yang sudah tersedia lokal tanpa menambah dependensi proyek.

- `light-1440-overview.png` dan `light-375-overview.png`: halaman percontohan.
- `{light,dark}-{375,768,1440}-*.png`: matriks screenshot route.
- `verified-375-settings.png`, `verified-375-token-saver.png`, `verified-375-me.png`: pemeriksaan ulang setelah koreksi responsif.
- `chart-{light,dark}-{375,1440}.png` dan `charts.json`: grafik berisi fixture dan warna sumbu hasil render.
- `checks.json`: hasil smoke test dan pengukuran lebar konten.
- `extended.json`: pengukuran 320px, zoom, state, dan reduced motion. Sampel kontras awal dalam file ini ditimpa secara konseptual oleh run `contrast.json`, yang menunggu transisi warna selesai dan mengecualikan teks visually hidden.
- `contrast.json`: hasil kontras setelah transisi tema selesai.
- `matrix.json` dan `gallery.png`: hasil matriks serta contact sheet peninjauan.

File bukti lokal tidak dikomit dan tidak menjadi bagian bundle aplikasi.

## Tindak lanjut: grafik stiker dan kartu untuk sosial media

Grafik bersama di Usage dan My Usage kini memiliki bidang plot, legend, dan tooltip dengan bingkai potongan kertas. Bar memakai outline tinta dan warna primary yang mengikuti tema. Canvas provider di Usage memakai node router kobalt dan node provider seperti stiker, dengan interaksi pan/zoom serta reduced motion yang tetap tersedia.

My Usage memiliki `UsageShareCard.vue`, dengan pilihan landscape 1200 x 630 dan square 1080 x 1080. Susunan data mengikuti contoh pengguna: total token sebagai fokus, request, rata-rata token per upstream request, biaya USD, serta empat top model dengan persentase token dan jumlah model tambahan. Periode mengikuti respons usage, dan urutan model dihitung dari prompt + completion tokens. Gambar memakai palet kertas yang sama pada kedua tema aplikasi.

PNG dibuat di browser dari SVG lokal yang memakai logo Alnair, tanpa dependensi ekspor tambahan. Download dan share memakai file yang sudah disiapkan. File sharing membuka menu perangkat jika didukung; jika tidak didukung atau gagal, file diunduh. Membatalkan menu share tidak memicu download. Credential, prefix key, dan alamat router tidak diserialisasikan ke gambar.

Validasi tindak lanjut:

- `pnpm test --maxWorkers=1`: 35 file, 264 tes lulus. Tes tambahan mencakup XML escaping, pemilihan field ekspor, akurasi total/periode dan ranking model, perubahan format, kesiapan PNG, fallback share, dan pembatalan.
- `pnpm run check`, `cargo fmt --all`, serta `cargo clippy --workspace --all-targets -- -D warnings`: lulus.
- Chromium pada Usage dan My Usage, 375/1440px dalam dua tema: delapan kombinasi tanpa runtime error atau overflow dokumen/konten utama.
- File hasil download diperiksa sebagai PNG dengan signature dan dimensi aktual 1200 x 630 serta 1080 x 1080. Preview dan hasil PNG juga diperiksa secara visual.
- Payload Web Share diuji memakai stub API browser: satu file PNG sesuai format. Pengujian tidak mengirim posting ke layanan sosial atau membuka aplikasi perangkat nyata.
- Bukti lokal: `share-checks.json`, `share-preview-{light,dark}-{375,1440}.png`, `sticker-chart-*.png`, `sticker-topology-*.png`, `usage-share-download.png`, dan `usage-share-square.png` dalam `.dev-home/ui-review/`.

Delivery Gate tindak lanjut: Hard Gate lulus pada cakupan render, error/loading, ekspor, keyboard format picker, dan pemeriksaan di atas; Purpose-Gate memakai alasan desain yang ditambahkan di DESIGN.md; Liveliness tetap ENERGY 3 / RHYTHM 3 / MOTION 1; Craftsmanship lulus pada kedua tema dan format ekspor. Fixture pada gambar contoh ditandai melalui nama key `Fixture / Development`.
