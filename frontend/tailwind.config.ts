export default {
    theme: {
        extend: {
            borderWidth: {
                title: '0.1rem',
            },
            boxShadow: {
                '3xl': '0 1em 5em rgba(0, 0, 0, 0.3)',
                glow: '0 0 10px rgba(0, 0, 0, 0.3)',
            },
            colors: {
                'my-gray': 'var(--my-gray)',
            },
            fontFamily: {
                sans: ['Source Sans Pro', 'Segoe UI', 'Helvetica Neue', 'Arial', 'sans-serif'],
            },
            fontSize: {
                sm: '14px',
                base: '15px',
                lg: '20px',
                xl: '24px',
            },
            screens: {
                xs: '500px',
                '2sm': '825px',
                '2md': '876px',
                '4xl': { min: '1971px' },
            },
            transitionProperty: {
                height: 'height',
            },
        },
    },
    safelist: ['alert-success', 'alert-warning', 'alert-info', 'alert-error'],
}
