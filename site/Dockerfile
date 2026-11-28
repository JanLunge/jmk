# The POG website: one static page, served by nginx.
FROM nginx:1.29-alpine
COPY deploy/nginx.conf /etc/nginx/conf.d/default.conf
COPY index.html icon.svg editor.webp /usr/share/nginx/html/
COPY fonts /usr/share/nginx/html/fonts
EXPOSE 80
