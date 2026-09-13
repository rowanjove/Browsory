/** @type {import('tailwindcss').Config} */
export default {
  darkMode: "class",
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      borderRadius: {
        DEFAULT: "4px",
        sm: "3px",
        md: "6px",
        lg: "8px",
      },
      fontSize: {
        xxs: "0.6875rem", // 11px 高密度
      },
    },
  },
  plugins: [],
}
