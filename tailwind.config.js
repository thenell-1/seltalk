/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./src/**/*.{vue,js,ts,jsx,tsx}",
    "./index.html",
  ],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // NOTE 主色调参考 Trae IDE：紫蓝色系
        primary: {
          50: "#EEF2FF",
          100: "#E0E7FF",
          200: "#C7D2FE",
          300: "#A5B4FC",
          400: "#818CF8",
          500: "#6366F1",
          600: "#4F46E5",
          700: "#4338CA",
          800: "#3730A3",
          900: "#312E81",
        },
      },
      fontFamily: {
        sans: ['"Microsoft YaHei"', '"PingFang SC"', '"Helvetica Neue"', "sans-serif"],
      },
    },
  },
  plugins: [],
};
